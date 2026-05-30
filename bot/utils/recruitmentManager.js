const {
    ActionRowBuilder,
    AttachmentBuilder,
    ButtonBuilder,
    ButtonStyle,
    ChannelType,
    EmbedBuilder,
    PermissionFlagsBits
} = require("discord.js");
const crypto = require("crypto");
const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");
const { incrementTeamCount } = require("./memberCountManager");
const { analyzeRecruitmentLicense } = require("./recruitmentOcr");
const { queueTeamRoleAssignment } = require("./teamRoleScheduler");
const {
    appendRecruitmentLog,
    getTicket,
    listTickets,
    saveTicket,
    updateTicket
} = require("./recruitmentStore");

const APPLY_BUTTON_ID = "recruitment:apply";
const EVENT_YES_PREFIX = "recruitment:event:yes:";
const EVENT_NO_PREFIX = "recruitment:event:no:";
const CLAIM_ID = "recruitment:claim";
const CLOSE_ID = "recruitment:close";
const CLOSE_TEAM_PREFIX = "recruitment:close-team:";
const TUTORIAL_PREFIX = "recruitment:tutorial:";
const SESSION_TIMEOUT_MS = 10 * 60 * 1000;
const IMAGE_EXT_RE = /\.(png|jpe?g|webp|gif)$/i;
const activeSessions = new Map();

const RECRUITMENT_OUTCOMES = [
    { id: "discord", label: "Discord", team: "Discord", style: ButtonStyle.Success },
    { id: "discord2", label: "Discord\u00b2", team: "Discord\u00b2", style: ButtonStyle.Success },
    { id: "discord3", label: "Discord 3\u2122", team: "Discord 3\u2122", style: ButtonStyle.Success },
    { id: "nascar-dc", label: "Nascar DC", team: "Nascar DC", style: ButtonStyle.Success },
    { id: "rejected", label: "Rejected", team: "", style: ButtonStyle.Danger }
];

function sessionToken() {
    return crypto.randomBytes(8).toString("hex");
}

function findActiveSession(guildId, userId) {
    return [...activeSessions.values()].find(session =>
        session.guildId === guildId &&
        session.userId === userId
    ) || null;
}

function rememberSession(session) {
    session.timeout = setTimeout(() => activeSessions.delete(session.token), SESSION_TIMEOUT_MS * 3);
    activeSessions.set(session.token, session);
}

function deleteSession(token) {
    const session = activeSessions.get(token);
    if (session?.timeout) clearTimeout(session.timeout);
    activeSessions.delete(token);
}

function recruiterRoleId(config = null) {
    return config?.recruitment?.recruiterRoleId ||
        config?.bot?.recruiterRoleId ||
        process.env.RECRUITER_ROLE_ID ||
        process.env.RECRUITMENT_RECRUITER_ROLE_ID ||
        "";
}

function isImageAttachment(attachment) {
    const contentType = String(attachment.contentType || "");
    return contentType.startsWith("image/") || IMAGE_EXT_RE.test(attachment.name || attachment.url || "");
}

function extractImageAttachments(message) {
    return [...message.attachments.values()]
        .filter(isImageAttachment)
        .slice(0, 10)
        .map(attachment => ({
            id: attachment.id,
            name: attachment.name || "screenshot",
            url: attachment.url,
            proxyUrl: attachment.proxyURL || "",
            contentType: attachment.contentType || "",
            size: attachment.size || 0
        }));
}

function normalizeCommandAttachment(attachment) {
    if (!attachment?.url) return null;

    return {
        id: attachment.id || `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        name: attachment.name || "screenshot",
        url: attachment.url,
        proxyUrl: attachment.proxyURL || attachment.proxyUrl || "",
        contentType: attachment.contentType || "",
        size: attachment.size || 0
    };
}

function displayTag(user) {
    return user.tag || user.username || user.id;
}

function safeThreadName(user) {
    return (user.username || user.globalName || "applicant")
        .replace(/[\r\n\t]+/g, " ")
        .replace(/^-+|-+$/g, "")
        .trim()
        .slice(0, 90) || "applicant";
}

function safeTicketName(value) {
    return String(value || "")
        .toLowerCase()
        .replace(/[^a-z0-9 _-]+/g, "")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 90);
}

function colorToNumber(color) {
    return Number.parseInt(String(color || "#0f766e").replace("#", ""), 16);
}

function buildPanelPayload(config) {
    const recruitment = config.recruitment;
    const embed = new EmbedBuilder()
        .setTitle(recruitment.panelTitle)
        .setDescription(recruitment.panelDescription)
        .setColor(colorToNumber(recruitment.panelColor))
        .setTimestamp();

    const row = new ActionRowBuilder().addComponents(
        new ButtonBuilder()
            .setCustomId(APPLY_BUTTON_ID)
            .setLabel("Apply!")
            .setStyle(ButtonStyle.Primary)
    );

    return { embeds: [embed], components: [row] };
}

function safeAttachmentName(name, fallback) {
    const clean = String(name || fallback || "screenshot.png")
        .replace(/[/\\?%*:|"<>]/g, "-")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 80);

    return clean || fallback || "screenshot.png";
}

function screenshotDmUserId(config) {
    return config?.recruitment?.screenshotDmUserId ||
        process.env.RECRUITMENT_SCREENSHOT_DM_USER_ID ||
        "";
}

function teamOutcomeId(index) {
    return `team:${index}`;
}

function teamNameFromOutcomeId(outcomeId, config) {
    if (!String(outcomeId || "").startsWith("team:")) return "";
    const index = Number.parseInt(outcomeId.slice("team:".length), 10);
    if (!Number.isInteger(index)) return "";
    return config?.recruitment?.teams?.[index] || "";
}

function responsePayload(payload) {
    if (typeof payload === "string") return { content: payload, ephemeral: true };
    return { ...payload, ephemeral: true };
}

async function respondEphemeral(interaction, payload) {
    const data = responsePayload(payload);

    if (interaction.deferred) {
        const { ephemeral, ...editable } = data;
        return interaction.editReply(editable);
    }
    if (interaction.replied) return interaction.followUp(data);
    return interaction.reply(data);
}

async function acknowledgeCloseSelection(interaction, payload) {
    const data = responsePayload(payload);

    if (interaction.isButton?.() && !interaction.deferred && !interaction.replied) {
        const { ephemeral, ...editable } = data;
        return interaction.update({ ...editable, components: data.components || [] });
    }

    return respondEphemeral(interaction, data);
}

async function fetchPanelMessage(channel, messageId) {
    if (!messageId || !channel.messages?.fetch) return null;

    try {
        return await channel.messages.fetch(messageId);
    } catch {
        return null;
    }
}

async function cleanRecruitmentPanelChannel(client, config = null) {
    const activeConfig = config || await loadDashboardConfig();
    const recruitment = activeConfig.recruitment;
    if (!recruitment.panelChannelId) return { skipped: true, reason: "Recruitment panel channel is not configured." };
    if (!recruitment.panelMessageId) return { skipped: true, reason: "Recruitment panel message is not configured." };

    const channel = await client.channels.fetch(recruitment.panelChannelId).catch(() => null);
    if (!channel?.isTextBased?.() || !channel.messages?.fetch) {
        return { skipped: true, reason: "Recruitment panel channel is not text based." };
    }

    let deleted = 0;
    let scanned = 0;
    let before;

    while (scanned < 500) {
        const messages = await channel.messages.fetch({ limit: 100, before }).catch(() => null);
        if (!messages?.size) break;

        scanned += messages.size;
        before = messages.last()?.id;
        for (const message of messages.values()) {
            if (message.id === recruitment.panelMessageId) continue;
            await message.delete().then(() => {
                deleted += 1;
            }).catch(() => null);
        }

        if (messages.size < 100) break;
    }

    return { deleted, channelId: channel.id };
}

async function ensureRecruitmentPanel(client) {
    const config = await loadDashboardConfig();
    const recruitment = config.recruitment;

    if (!recruitment.enabled) {
        return { skipped: true, reason: "Recruitment is disabled." };
    }

    if (!recruitment.panelChannelId) {
        return { skipped: true, reason: "Recruitment panel channel is not configured." };
    }

    const channel = await client.channels.fetch(recruitment.panelChannelId).catch(() => null);
    if (!channel?.isTextBased?.() || !channel.messages?.fetch) {
        return { skipped: true, reason: "Recruitment panel channel is not text based." };
    }

    const payload = buildPanelPayload(config);
    let message = await fetchPanelMessage(channel, recruitment.panelMessageId);
    const created = !message;

    if (message?.author?.id !== client.user.id) message = null;

    if (message) {
        await message.edit(payload);
    } else {
        message = await channel.send(payload);
    }

    if (message.id !== recruitment.panelMessageId) {
        await saveDashboardConfig({
            ...config,
            recruitment: {
                ...recruitment,
                panelMessageId: message.id
            }
        });
    }

    await cleanRecruitmentPanelChannel(client, {
        ...config,
        recruitment: {
            ...recruitment,
            panelMessageId: message.id
        }
    }).catch(error => {
        console.error("Failed to clean recruitment panel channel:", error.message);
    });

    return {
        created,
        channelId: channel.id,
        messageId: message.id
    };
}

function buildEventDecisionRow(token) {
    return new ActionRowBuilder().addComponents(
        new ButtonBuilder()
            .setCustomId(`${EVENT_YES_PREFIX}${token}`)
            .setLabel("Yes")
            .setStyle(ButtonStyle.Success),
        new ButtonBuilder()
            .setCustomId(`${EVENT_NO_PREFIX}${token}`)
            .setLabel("No")
            .setStyle(ButtonStyle.Secondary)
    );
}

function buildCloseOutcomeRows(config) {
    const rows = [];
    let current = new ActionRowBuilder();
    const outcomes = [
        ...(config?.recruitment?.teams || [])
            .filter(Boolean)
            .slice(0, 24)
            .map((team, index) => ({
                id: teamOutcomeId(index),
                label: team,
                style: ButtonStyle.Success
            })),
        { id: "rejected", label: "Rejected", style: ButtonStyle.Danger }
    ];

    for (const outcome of outcomes) {
        if (current.components.length === 5) {
            rows.push(current);
            current = new ActionRowBuilder();
        }

        current.addComponents(
            new ButtonBuilder()
                .setCustomId(`${CLOSE_TEAM_PREFIX}${outcome.id}`)
                .setLabel(outcome.label)
                .setStyle(outcome.style)
        );
    }

    if (current.components.length) rows.push(current);
    return rows;
}

function buildTicketControls(config) {
    const rows = [
        new ActionRowBuilder().addComponents(
            new ButtonBuilder()
                .setCustomId(CLAIM_ID)
                .setLabel("Claim Ticket")
                .setStyle(ButtonStyle.Secondary),
            new ButtonBuilder()
                .setCustomId(CLOSE_ID)
                .setLabel("Close Ticket")
                .setStyle(ButtonStyle.Danger)
        )
    ];

    const tutorialButtons = config.recruitment.tutorials
        .filter(tutorial => tutorial.enabled)
        .slice(0, 5)
        .map(tutorial =>
            new ButtonBuilder()
                .setCustomId(`${TUTORIAL_PREFIX}${tutorial.id}`)
                .setLabel(`Send ${tutorial.label}`.slice(0, 80))
                .setStyle(ButtonStyle.Primary)
        );

    if (tutorialButtons.length) {
        rows.push(new ActionRowBuilder().addComponents(...tutorialButtons));
    }

    return rows;
}

function buildApplicationEmbeds(config, session) {
    const screenshots = [
        ...session.licenseAttachments,
        ...session.eventAttachments
    ];

    const firstScreenshot = screenshots[0];
    const links = screenshots
        .map((attachment, index) => `[${attachment.name || `Screenshot ${index + 1}`}](${attachment.url})`)
        .join("\n")
        .slice(0, 1000);

    const intro = config.recruitment.questionsIntro;
    const questions = config.recruitment.questions;
    const embed = new EmbedBuilder()
        .setTitle("Recruitment Application")
        .setDescription(`${intro}\n\n**Questions:**\n\n${questions}`)
        .setColor(colorToNumber(config.recruitment.panelColor))
        .addFields(
            { name: "Applicant", value: `<@${session.userId}>`, inline: true },
            { name: "Team Event Screenshots", value: session.eventAttachments.length ? "Provided" : "Not provided", inline: true },
            { name: "Screenshots", value: links || "No screenshots captured." }
        )
        .setTimestamp();

    if (firstScreenshot?.url) {
        embed.setImage(firstScreenshot.url);
    }

    const imageEmbeds = screenshots
        .slice(1, 9)
        .map((attachment, index) =>
            new EmbedBuilder()
                .setTitle(`Screenshot ${index + 2}`)
                .setURL(attachment.url)
                .setImage(attachment.url)
                .setColor(colorToNumber(config.recruitment.panelColor))
        );

    return [embed, ...imageEmbeds];
}

function threadTypeFor(channel, privateThreads) {
    if (channel.type === ChannelType.GuildAnnouncement) return ChannelType.AnnouncementThread;
    return privateThreads ? ChannelType.PrivateThread : ChannelType.PublicThread;
}

async function mirrorAttachmentsToDm(client, config, attachments, context) {
    const recipientId = screenshotDmUserId(config);
    if (!recipientId) {
        throw new Error("Screenshot DM user is not configured in the dashboard.");
    }

    const recipient = await client.users.fetch(recipientId).catch(() => null);
    if (!recipient) throw new Error("I could not find the configured screenshot DM user.");

    const dm = await recipient.createDM();
    const mirrored = [];

    for (let index = 0; index < attachments.length; index += 1) {
        const attachment = attachments[index];
        const response = await fetch(attachment.url);
        if (!response.ok) {
            throw new Error(`Could not download ${attachment.name || "screenshot"} (${response.status}).`);
        }

        const buffer = Buffer.from(await response.arrayBuffer());
        const name = safeAttachmentName(
            attachment.name,
            `${context.kind || "screenshot"}-${context.userId || "user"}-${index + 1}.png`
        );
        const sent = await dm.send({
            content: [
                `Recruitment ${context.kind || "screenshot"} upload`,
                `Applicant: <@${context.userId}> (${context.userTag || context.userId})`,
                context.guildId ? `Guild: ${context.guildId}` : ""
            ].filter(Boolean).join("\n"),
            files: [new AttachmentBuilder(buffer, { name })],
            allowedMentions: { parse: [] }
        });
        const mirroredAttachment = sent.attachments.first();
        if (!mirroredAttachment?.url) {
            throw new Error("Discord did not return a URL for the mirrored screenshot.");
        }

        mirrored.push({
            ...attachment,
            name,
            url: mirroredAttachment.url,
            proxyUrl: mirroredAttachment.proxyURL || "",
            contentType: mirroredAttachment.contentType || attachment.contentType || "",
            size: mirroredAttachment.size || attachment.size || buffer.length,
            source: "dm-mirror",
            dmUserId: recipientId,
            dmMessageId: sent.id,
            originalUrl: attachment.url
        });
    }

    return mirrored;
}

async function waitForImageMessage(channel, userId, onInvalid = null) {
    return new Promise((resolve, reject) => {
        let settled = false;
        const collector = channel.createMessageCollector({
            filter: message => message.author.id === userId && !message.author.bot,
            time: SESSION_TIMEOUT_MS
        });

        collector.on("collect", async message => {
            const attachments = extractImageAttachments(message);
            if (!attachments.length) {
                await message.delete().catch(() => null);
                if (onInvalid) await onInvalid().catch(() => null);
                return;
            }

            settled = true;
            collector.stop("found");
            resolve({ message, attachments });
        });

        collector.on("end", () => {
            if (!settled) reject(new Error("Timed out waiting for screenshots."));
        });
    });
}

async function userHasOpenTicket(config, userId) {
    const openTickets = await listTickets({ applicantId: userId, status: "open" });
    const maxOpen = Number(config.recruitment.maxOpenTicketsPerUser || 1);
    return openTickets.length >= maxOpen ? openTickets[0] : null;
}

async function collectLicense(interaction) {
    let config = await loadDashboardConfig();
    const existingTicket = await userHasOpenTicket(config, interaction.user.id);
    if (existingTicket) {
        await interaction.reply({
            content: `You already have an open application ticket: <#${existingTicket.threadId}>`,
            ephemeral: true
        });
        return;
    }

    if (findActiveSession(interaction.guildId, interaction.user.id)) {
        await interaction.reply({
            content: "You already have an application upload in progress. Finish that upload or wait for it to time out before pressing **Apply!** again.",
            ephemeral: true
        });
        return;
    }

    if (!screenshotDmUserId(config)) {
        await interaction.reply({
            content: "Recruitment screenshot storage is not configured yet. Ask a manager to set the screenshot DM user in the dashboard.",
            ephemeral: true
        });
        return;
    }

    if (!config.recruitment.panelMessageId && interaction.message?.id) {
        config = await saveDashboardConfig({
            ...config,
            recruitment: {
                ...config.recruitment,
                panelMessageId: interaction.message.id
            }
        });
    }

    const session = {
        token: sessionToken(),
        userId: interaction.user.id,
        userTag: displayTag(interaction.user),
        username: interaction.user.username || interaction.user.globalName || interaction.user.id,
        guildId: interaction.guildId,
        channelId: interaction.channelId,
        licenseAttachments: [],
        eventAttachments: [],
        startedAt: new Date().toISOString()
    };
    rememberSession(session);

    await interaction.reply({
        content: "Upload your in-game driver's license screenshot in this channel now. I will store it privately and remove your visible upload right away.",
        ephemeral: true
    });

    try {
        const upload = await waitForImageMessage(interaction.channel, interaction.user.id, () =>
            interaction.followUp({
                content: "That message did not include an image attachment, so I removed it. Please upload the driver's license screenshot as an image file.",
                ephemeral: true
            })
        );
        session.licenseAttachments = await mirrorAttachmentsToDm(interaction.client, config, upload.attachments, {
            kind: "driver license",
            userId: interaction.user.id,
            userTag: displayTag(interaction.user),
            guildId: interaction.guildId
        });
        await upload.message.delete().catch(() => null);
        await cleanRecruitmentPanelChannel(interaction.client, config).catch(() => null);

        await interaction.editReply({
            content: "Driver's license captured. Do you have team event score screenshots to add?",
            components: [buildEventDecisionRow(session.token)]
        });
    } catch (error) {
        deleteSession(session.token);
        await interaction.editReply({
            content: error.message.includes("Timed out")
                ? "Application timed out because no driver's license image was uploaded in time. Press **Apply!** again when you are ready."
                : `I could not process that screenshot: ${error.message}`,
            components: []
        }).catch(() => null);
    }
}

async function createApplicationThread(client, session) {
    const config = await loadDashboardConfig();
    const channel = await client.channels.fetch(session.channelId).catch(() => null);
    if (!channel?.threads?.create) {
        throw new Error("This channel does not support threads.");
    }

    const user = await client.users.fetch(session.userId).catch(() => ({
        id: session.userId,
        username: session.username || session.userTag || session.userId,
        tag: session.userTag || session.userId
    }));

    const threadType = threadTypeFor(channel, config.recruitment.privateThreads);
    const threadOptions = {
        name: safeThreadName(user),
        autoArchiveDuration: config.recruitment.threadAutoArchiveMinutes,
        type: threadType,
        reason: `Recruitment application from ${displayTag(user)}`
    };

    if (threadType === ChannelType.PrivateThread) {
        threadOptions.invitable = false;
    }

    const thread = await channel.threads.create(threadOptions);

    await thread.members.add(session.userId).catch(() => null);

    const roleId = recruiterRoleId(config);
    const intro = roleId
        ? `<@&${roleId}> New recruitment application from <@${session.userId}>.`
        : `New recruitment application from <@${session.userId}>. Configure the recruiter role in the dashboard to ping recruiters.`;

    await thread.send({
        content: intro,
        embeds: buildApplicationEmbeds(config, session),
        components: buildTicketControls(config)
    });

    const ticket = await saveTicket({
        threadId: thread.id,
        threadName: thread.name,
        channelId: channel.id,
        guildId: session.guildId,
        applicantId: session.userId,
        applicantTag: displayTag(user),
        applicantUsername: user.username || session.username || "",
        status: "open",
        claimedById: "",
        claimedByTag: "",
        addedUserIds: [],
        licenseAttachments: session.licenseAttachments,
        eventAttachments: session.eventAttachments,
        createdAt: new Date().toISOString()
    });

    await logAction(client, {
        type: "ticket",
        title: "Recruitment Ticket Created",
        message: `<@${session.userId}> opened a recruitment application thread.`,
        guildId: session.guildId,
        actorId: session.userId,
        actorTag: displayTag(user),
        metadata: { threadId: thread.id, channelId: channel.id }
    });

    return { thread, ticket };
}

function sessionFromEventButton(interaction, prefix) {
    const token = interaction.customId.slice(prefix.length);
    const session = activeSessions.get(token);
    if (!session || session.userId !== interaction.user.id) return null;
    return session;
}

async function completeWithoutEvents(interaction) {
    const session = sessionFromEventButton(interaction, EVENT_NO_PREFIX);
    if (!session) {
        await interaction.reply({
            content: "That application prompt expired. Press **Apply!** again when you are ready.",
            ephemeral: true
        });
        return;
    }

    await interaction.update({
        content: "Got it. Creating your application ticket now.",
        components: []
    });

    try {
        const { thread } = await createApplicationThread(interaction.client, session);
        deleteSession(session.token);
        await interaction.followUp({
            content: `Your application ticket has been created: <#${thread.id}>`,
            ephemeral: true
        });
    } catch (error) {
        deleteSession(session.token);
        await interaction.followUp({
            content: `I could not create the ticket: ${error.message}`,
            ephemeral: true
        });
    }
}

async function collectEventScreenshots(interaction) {
    const session = sessionFromEventButton(interaction, EVENT_YES_PREFIX);
    if (!session) {
        await interaction.reply({
            content: "That application prompt expired. Press **Apply!** again when you are ready.",
            ephemeral: true
        });
        return;
    }

    await interaction.update({
        content: "Upload the team event score screenshots in this channel now. I will store them privately and remove the visible upload right away.",
        components: []
    });

    try {
        const config = await loadDashboardConfig();
        const upload = await waitForImageMessage(interaction.channel, interaction.user.id, () =>
            interaction.followUp({
                content: "That message did not include an image attachment, so I removed it. Please upload the team event scores as image files.",
                ephemeral: true
            })
        );
        session.eventAttachments = await mirrorAttachmentsToDm(interaction.client, config, upload.attachments, {
            kind: "team event scores",
            userId: interaction.user.id,
            userTag: displayTag(interaction.user),
            guildId: interaction.guildId
        });
        await upload.message.delete().catch(() => null);
        await cleanRecruitmentPanelChannel(interaction.client, config).catch(() => null);

        const { thread } = await createApplicationThread(interaction.client, session);
        deleteSession(session.token);
        await interaction.followUp({
            content: `Your application ticket has been created: <#${thread.id}>`,
            ephemeral: true
        });
    } catch (error) {
        deleteSession(session.token);
        await interaction.followUp({
            content: error.message.includes("Timed out")
                ? "Application timed out because no team event screenshot was uploaded in time. Press **Apply!** again when you are ready."
                : `I could not process those screenshots: ${error.message}`,
            ephemeral: true
        }).catch(() => null);
    }
}

function memberCanRecruit(member, config = null) {
    if (!member) return false;
    const roleId = recruiterRoleId(config);
    const hasRole = roleId && member.roles?.cache?.has(roleId);
    const permissions = member.permissions;

    return Boolean(
        hasRole ||
        permissions?.has(PermissionFlagsBits.Administrator) ||
        permissions?.has(PermissionFlagsBits.ManageGuild) ||
        permissions?.has(PermissionFlagsBits.ManageThreads)
    );
}

async function requireRecruiter(interaction) {
    const config = await loadDashboardConfig();
    const member = interaction.member?.roles?.cache
        ? interaction.member
        : await interaction.guild.members.fetch(interaction.user.id).catch(() => null);

    if (memberCanRecruit(member, config)) return true;

    await respondEphemeral(interaction, "Only recruiters can use these ticket controls.").catch(() => null);
    return false;
}

async function getTicketContext(interaction) {
    const thread = interaction.channel;
    if (!thread?.isThread?.()) {
        await respondEphemeral(interaction, "Recruitment ticket commands must be used inside a ticket thread.");
        return null;
    }

    const ticket = await getTicket(thread.id);
    if (!ticket) {
        await respondEphemeral(interaction, "I could not find a ticket record for this thread.");
        return null;
    }

    return { thread, ticket };
}

const SCREENSHOT_TARGETS = {
    license: {
        field: "licenseAttachments",
        label: "driver's license",
        plural: "driver's license screenshots"
    },
    event: {
        field: "eventAttachments",
        label: "team event scores",
        plural: "team event score screenshots"
    }
};

function screenshotTarget(type) {
    return SCREENSHOT_TARGETS[type] || null;
}

function cleanScreenshotList(items = []) {
    const seen = new Set();
    return items
        .filter(item => item?.url && !seen.has(item.url) && seen.add(item.url))
        .map(item => ({ ...item }));
}

function refreshApplicantImages(ticket, nextLicense, nextEvents) {
    const previousManagedUrls = new Set([
        ...(ticket.licenseAttachments || []),
        ...(ticket.eventAttachments || [])
    ].map(item => item?.url).filter(Boolean));
    const managed = cleanScreenshotList([...nextLicense, ...nextEvents]);
    const retained = (ticket.applicantThreadImages || [])
        .filter(item => item?.url && !previousManagedUrls.has(item.url));

    return cleanScreenshotList([...managed, ...retained]);
}

function screenshotListText(ticket, target) {
    const screenshots = ticket[target.field] || [];
    if (!screenshots.length) return `No ${target.plural} are saved for this ticket.`;

    return screenshots
        .map((screenshot, index) => {
            const label = screenshot.name || `Screenshot ${index + 1}`;
            const actor = screenshot.uploadedById ? ` added by <@${screenshot.uploadedById}>` : "";
            return `${index + 1}. [${label}](${screenshot.url})${actor}`;
        })
        .join("\n")
        .slice(0, 1900);
}

async function normalizeTicketScreenshots(interaction, type, attachments, ticket = null) {
    const target = screenshotTarget(type);
    if (!target) throw new Error("Unknown screenshot type.");

    const normalized = attachments
        .map(normalizeCommandAttachment)
        .filter(Boolean);

    if (!normalized.length) {
        throw new Error("Attach at least one image.");
    }

    const invalid = normalized.find(attachment => !isImageAttachment(attachment));
    if (invalid) {
        throw new Error(`${invalid.name || "Attachment"} is not an image.`);
    }

    const config = await loadDashboardConfig();
    const mirrored = await mirrorAttachmentsToDm(interaction.client, config, normalized, {
        kind: target.label,
        userId: ticket?.applicantId || interaction.user.id,
        userTag: ticket?.applicantTag || displayTag(interaction.user),
        guildId: interaction.guildId
    });
    const now = new Date().toISOString();

    return mirrored.map(attachment => ({
        ...attachment,
        uploadedById: interaction.user.id,
        uploadedByTag: displayTag(interaction.user),
        uploadedAt: now,
        managedType: type
    }));
}

async function updateTicketScreenshots(interaction, type, updater, actionLabel, existingContext = null) {
    if (!existingContext && !(await requireRecruiter(interaction))) return null;

    const context = existingContext || await getTicketContext(interaction);
    if (!context) return null;

    const { thread, ticket } = context;
    if (ticket.status === "deleted") {
        await respondEphemeral(interaction, "This ticket was deleted.");
        return null;
    }

    const target = screenshotTarget(type);
    if (!target) {
        await respondEphemeral(interaction, "Unknown screenshot type.");
        return null;
    }

    const current = cleanScreenshotList(ticket[target.field] || []);
    let next;
    try {
        next = cleanScreenshotList(await updater(current, target));
    } catch (error) {
        await respondEphemeral(interaction, error.message);
        return null;
    }
    const nextLicense = target.field === "licenseAttachments" ? next : cleanScreenshotList(ticket.licenseAttachments || []);
    const nextEvents = target.field === "eventAttachments" ? next : cleanScreenshotList(ticket.eventAttachments || []);
    const patch = {
        [target.field]: next,
        applicantThreadImages: refreshApplicantImages(ticket, nextLicense, nextEvents),
        screenshotsUpdatedAt: new Date().toISOString(),
        screenshotsUpdatedById: interaction.user.id,
        screenshotsUpdatedByTag: displayTag(interaction.user)
    };

    if (ticket.licenseAnalysis) {
        patch.licenseAnalysis = null;
        patch.licenseAnalysisInvalidatedAt = patch.screenshotsUpdatedAt;
    }

    const updated = await updateTicket(thread.id, patch);
    await thread.send(`${target.plural} ${actionLabel} by <@${interaction.user.id}>.`).catch(() => null);
    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Ticket Screenshots Updated",
        message: `<@${interaction.user.id}> ${actionLabel} ${target.plural} for <@${ticket.applicantId}>.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: {
            threadId: thread.id,
            screenshotType: type,
            screenshotCount: next.length
        }
    });

    await respondEphemeral(interaction, [
        `${target.plural} ${actionLabel}.`,
        screenshotListText(updated || { ...ticket, [target.field]: next }, target)
    ].join("\n\n"));

    return updated;
}

async function listTicketScreenshots(interaction, type) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const target = screenshotTarget(type);
    if (!target) {
        await respondEphemeral(interaction, "Unknown screenshot type.");
        return;
    }

    await respondEphemeral(interaction, screenshotListText(context.ticket, target));
}

async function addTicketScreenshots(interaction, type, attachments) {
    if (!(await requireRecruiter(interaction))) return null;

    const context = await getTicketContext(interaction);
    if (!context) return null;

    let additions;
    try {
        additions = await normalizeTicketScreenshots(interaction, type, attachments, context.ticket);
    } catch (error) {
        await respondEphemeral(interaction, error.message);
        return null;
    }

    return updateTicketScreenshots(
        interaction,
        type,
        current => [...current, ...additions],
        additions.length === 1 ? "added" : `added (${additions.length})`,
        context
    );
}

async function removeTicketScreenshot(interaction, type, index) {
    const targetIndex = Number(index) - 1;
    return updateTicketScreenshots(interaction, type, current => {
        if (!Number.isInteger(targetIndex) || targetIndex < 0 || targetIndex >= current.length) {
            throw new Error(`Pick an index between 1 and ${Math.max(1, current.length)}.`);
        }

        const next = [...current];
        next.splice(targetIndex, 1);
        return next;
    }, "removed");
}

async function changeTicketScreenshot(interaction, type, index, attachment) {
    if (!(await requireRecruiter(interaction))) return null;

    const context = await getTicketContext(interaction);
    if (!context) return null;

    let replacements;
    try {
        replacements = await normalizeTicketScreenshots(interaction, type, [attachment], context.ticket);
    } catch (error) {
        await respondEphemeral(interaction, error.message);
        return null;
    }

    const targetIndex = Number(index) - 1;

    return updateTicketScreenshots(interaction, type, current => {
        if (!Number.isInteger(targetIndex) || targetIndex < 0 || targetIndex >= current.length) {
            throw new Error(`Pick an index between 1 and ${Math.max(1, current.length)}.`);
        }

        const next = [...current];
        next[targetIndex] = replacements[0];
        return next;
    }, "changed", context);
}

async function claimTicket(interaction) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    if (ticket.status === "closed") {
        await respondEphemeral(interaction, "This ticket is already closed.");
        return;
    }

    if (ticket.claimedById) {
        await respondEphemeral(interaction, ticket.claimedById === interaction.user.id
            ? "You have already claimed this ticket."
            : `This ticket is already claimed by <@${ticket.claimedById}>.`);
        return;
    }

    const updated = await updateTicket(thread.id, {
        claimedById: interaction.user.id,
        claimedByTag: displayTag(interaction.user)
    });

    await thread.send(`Ticket claimed by <@${interaction.user.id}>.`);
    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Ticket Claimed",
        message: `<@${interaction.user.id}> claimed the ticket for <@${ticket.applicantId}>.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: { threadId: thread.id }
    });

    await respondEphemeral(interaction, `Ticket claimed for ${updated.applicantTag}.`);
}

async function sendTutorial(interaction, tutorialId) {
    if (!(await requireRecruiter(interaction))) return;

    const config = await loadDashboardConfig();
    const tutorial = config.recruitment.tutorials.find(item => item.id === tutorialId && item.enabled);

    if (!tutorial) {
        await respondEphemeral(interaction, "That tutorial is not configured anymore.");
        return;
    }

    if (!tutorial.videoUrl) {
        await respondEphemeral(interaction, `No video has been uploaded for **${tutorial.label}** yet.`);
        return;
    }

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    const applicantMention = ticket?.applicantId ? `<@${ticket.applicantId}> ` : "";

    await thread.send({
        content: `${applicantMention}${tutorial.description || tutorial.label}\n${tutorial.videoUrl}`
    });

    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Tutorial Sent",
        message: `<@${interaction.user.id}> sent **${tutorial.label}** in a ticket.`,
        guildId: interaction.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket?.applicantId || "",
        targetTag: ticket?.applicantTag || "",
        metadata: { threadId: thread.id, tutorialId }
    });

    await respondEphemeral(interaction, `Sent **${tutorial.label}** to the ticket.`);
}

function renderInviteMessage(template, ticket, inviteUrl, serverName) {
    const userMention = `<@${ticket.applicantId}>`;
    let content = String(template || "{user} Join **{server}** here: {invite}")
        .replaceAll("{user}", userMention)
        .replaceAll("{invite}", inviteUrl)
        .replaceAll("{server}", serverName || "the server");

    if (!content.includes(userMention)) content = `${userMention} ${content}`;
    if (!content.includes(inviteUrl)) content = `${content}\n${inviteUrl}`;
    return content.slice(0, 2000);
}

async function sendInviteToApplicant(interaction) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    if (ticket.status === "closed" || ticket.status === "archived" || ticket.status === "deleted") {
        await respondEphemeral(interaction, "This ticket is already closed.");
        return;
    }

    const config = await loadDashboardConfig();
    const inviteChannelId = config.recruitment.inviteChannelId;
    if (!inviteChannelId) {
        await respondEphemeral(interaction, "Configure the destination invite channel in the dashboard first.");
        return;
    }

    const channel = await interaction.client.channels.fetch(inviteChannelId).catch(() => null);
    if (!channel?.createInvite) {
        await respondEphemeral(interaction, "I could not access an invite-capable channel for the destination server.");
        return;
    }

    if (config.recruitment.inviteGuildId && channel.guild?.id !== config.recruitment.inviteGuildId) {
        await respondEphemeral(interaction, "The configured invite channel does not belong to the configured destination server.");
        return;
    }

    const invite = await channel.createInvite({
        maxAge: 0,
        maxUses: 1,
        unique: true,
        reason: `Recruitment invite for ${ticket.applicantTag || ticket.applicantId}`
    });
    const inviteUrl = invite.url || `https://discord.gg/${invite.code}`;
    const content = renderInviteMessage(
        config.recruitment.inviteMessage,
        ticket,
        inviteUrl,
        channel.guild?.name || "the server"
    );

    await thread.send({
        content,
        allowedMentions: { users: [ticket.applicantId], roles: [] }
    });

    await updateTicket(thread.id, {
        lastInviteAt: new Date().toISOString(),
        lastInviteById: interaction.user.id,
        lastInviteByTag: displayTag(interaction.user),
        lastInviteGuildId: channel.guild?.id || "",
        lastInviteChannelId: channel.id
    });

    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Invite Sent",
        message: `<@${interaction.user.id}> sent a single-use invite to <@${ticket.applicantId}>.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: {
            threadId: thread.id,
            inviteGuildId: channel.guild?.id || "",
            inviteChannelId: channel.id
        }
    });

    await respondEphemeral(interaction, `Sent a single-use invite to ${ticket.applicantTag || ticket.applicantId}.`);
}

async function startClose(interaction) {
    if (!(await requireRecruiter(interaction))) return;

    const config = await loadDashboardConfig();
    await respondEphemeral(interaction, {
        content: "Select the final recruitment outcome for this applicant.",
        components: buildCloseOutcomeRows(config)
    });
}

function outcomeFromId(outcomeId, config) {
    if (outcomeId === "rejected") return { id: "rejected", label: "Rejected", team: "", style: ButtonStyle.Danger };

    const configuredTeam = teamNameFromOutcomeId(outcomeId, config);
    if (configuredTeam) {
        const team = (config?.recruitment?.teams || []).find(item => item === configuredTeam) || configuredTeam;
        return { id: outcomeId, label: team, team, style: ButtonStyle.Success };
    }

    return RECRUITMENT_OUTCOMES.find(team => team.id === outcomeId) || null;
}

async function fetchThreadMessages(thread, limit = 250) {
    const collected = [];
    let before;

    while (collected.length < limit) {
        const batch = await thread.messages.fetch({ limit: Math.min(100, limit - collected.length), before }).catch(() => null);
        if (!batch?.size) break;

        collected.push(...batch.values());
        before = batch.last()?.id;
        if (batch.size < 100) break;
    }

    return collected.sort((a, b) => a.createdTimestamp - b.createdTimestamp);
}

function transcriptLine(message) {
    const author = message.author?.tag || message.author?.username || message.author?.id || "Unknown";
    const attachments = [...message.attachments.values()].map(item => item.url);
    const embeds = message.embeds.flatMap(embed => [
        embed.title,
        embed.description,
        embed.url,
        embed.image?.url,
        embed.thumbnail?.url,
        ...(embed.fields || []).flatMap(field => [`${field.name}: ${field.value}`])
    ]).filter(Boolean);
    const content = String(message.content || "").replace(/\s+/g, " ").trim();
    const extras = [...attachments, ...embeds].join(" ");

    return `[${message.createdAt.toISOString()}] ${author}: ${content}${extras ? ` ${extras}` : ""}`.trim();
}

async function createTranscript(thread, limit = 250) {
    if (!thread?.messages?.fetch) return "";

    const collected = await fetchThreadMessages(thread, limit);
    return collected
        .map(transcriptLine)
        .join("\n")
        .slice(0, 300000);
}

async function collectApplicantThreadImages(thread, applicantId, limit = 250) {
    if (!thread?.messages?.fetch) return [];

    const seen = new Set();
    const messages = await fetchThreadMessages(thread, limit);
    const images = [];

    for (const message of messages) {
        if (message.author?.id !== applicantId) continue;

        const attachments = extractImageAttachments(message);
        const embedImages = message.embeds.flatMap(embed => [embed.image?.url, embed.thumbnail?.url]).filter(Boolean);
        const candidates = [
            ...attachments.map(attachment => ({
                id: attachment.id,
                name: attachment.name,
                url: attachment.url,
                contentType: attachment.contentType,
                size: attachment.size
            })),
            ...embedImages.map((url, index) => ({
                id: `${message.id}-embed-${index}`,
                name: "embedded-image",
                url,
                contentType: "",
                size: 0
            }))
        ];

        for (const image of candidates) {
            if (!image.url || seen.has(image.url)) continue;
            seen.add(image.url);
            images.push({
                ...image,
                messageId: message.id,
                authorId: message.author.id,
                createdAt: message.createdAt.toISOString()
            });
        }
    }

    return images.slice(0, 50);
}

function embedFieldValue(value, fallback = "Not detected", maxLength = 1024) {
    const text = String(value || "")
        .replace(/\s+/g, " ")
        .trim();
    return (text || fallback).slice(0, maxLength);
}

function firstImageAttachment(attachments = []) {
    return attachments.find(attachment => attachment?.url) || null;
}

function formatRecruitmentEventScores(eventScores = []) {
    const lines = eventScores
        .filter(Boolean)
        .slice(0, 8)
        .map(event => {
            const eventName = embedFieldValue(event.eventName, "Team Event", 120);
            const details = [
                event.rank ? `rank ${event.rank}` : "",
                event.eventPoints ? `points ${event.eventPoints}` : "",
                event.score ? `score ${event.score}` : ""
            ].filter(Boolean).join(", ");
            return details ? `**${eventName}** - ${details}` : `**${eventName}** - score not detected`;
        });

    return lines.length ? lines.join("\n").slice(0, 1024) : "Not detected";
}

async function sendRecruitmentLog(client, config, ticket, applicantImages = [], closeDetails = null) {
    const channelId = config.recruitment.logChannelId || config.logging.channelId;
    if (!channelId || !closeDetails) return;

    const channel = await client.channels.fetch(channelId).catch(() => null);
    if (!channel?.isTextBased?.()) return;

    const licenseImage = firstImageAttachment(ticket.licenseAttachments || []) ||
        firstImageAttachment(applicantImages);
    const previousTeam = closeDetails.previousTeam || closeDetails.sourceTeam || "";
    const embed = new EmbedBuilder()
        .setTitle("Recruitment Closing Log")
        .setColor(colorToNumber(config.recruitment.panelColor))
        .addFields(
            { name: "Discord user ID", value: embedFieldValue(closeDetails.discordId || ticket.applicantId, "Unknown"), inline: true },
            { name: "Applicant", value: ticket.applicantId ? `<@${ticket.applicantId}>` : "Unknown", inline: true },
            { name: "In-game name", value: embedFieldValue(closeDetails.inGameName), inline: true },
            { name: "Previous team", value: embedFieldValue(previousTeam), inline: true },
            { name: "Team joined", value: embedFieldValue(closeDetails.acceptedTeam || ticket.team || "Rejected", "Rejected"), inline: true },
            { name: "Garage power", value: embedFieldValue(closeDetails.garagePower), inline: true },
            { name: "Team event scores", value: formatRecruitmentEventScores(closeDetails.eventScores), inline: false },
            { name: "Closed by", value: ticket.closedById ? `<@${ticket.closedById}>` : "Unknown", inline: true }
        )
        .setTimestamp();

    if (licenseImage?.url) {
        embed.setImage(licenseImage.url);
    }

    if (closeDetails.error) {
        embed.addFields({ name: "Gemini note", value: String(closeDetails.error).slice(0, 1024), inline: false });
    }

    await channel.send({ embeds: [embed], allowedMentions: { parse: [] } });
}

async function finishClose(interaction, outcomeId) {
    if (interaction.isButton?.() && !interaction.deferred && !interaction.replied) {
        await interaction.deferUpdate();
    }

    if (!(await requireRecruiter(interaction))) return;

    const config = await loadDashboardConfig();
    const outcome = outcomeFromId(outcomeId, config);
    if (!outcome) {
        await respondEphemeral(interaction, "Unknown recruitment outcome.");
        return;
    }

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    if (ticket.status === "closed") {
        await respondEphemeral(interaction, "This ticket is already closed.");
        return;
    }

    const accepted = Boolean(outcome.team);
    const closedAt = new Date().toISOString();
    const transcriptText = config.recruitment.transcriptOnClose ? await createTranscript(thread) : "";
    const transcriptLines = transcriptText ? transcriptText.split("\n").filter(Boolean).length : 0;
    const applicantThreadImages = [
        ...(ticket.licenseAttachments || []),
        ...(ticket.eventAttachments || []),
        ...(await collectApplicantThreadImages(thread, ticket.applicantId))
    ].filter((image, index, list) => image?.url && list.findIndex(item => item.url === image.url) === index);
    const licenseAnalysis = await analyzeRecruitmentLicense({
        ...ticket,
        team: accepted ? outcome.team : "Rejected"
    }, config).catch(error => ({
        discordId: ticket.applicantId,
        inGameName: "",
        sourceTeam: "",
        previousTeam: "",
        acceptedTeam: accepted ? outcome.team : "Rejected",
        garagePower: "",
        eventScores: [],
        rawText: "",
        error: error.message
    }));
    licenseAnalysis.acceptedTeam = accepted ? outcome.team : "Rejected";

    const updatedTicket = await updateTicket(thread.id, {
        status: "closed",
        closedAt,
        closedById: interaction.user.id,
        closedByTag: displayTag(interaction.user),
        archivedAt: closedAt,
        archivedById: interaction.user.id,
        archivedByTag: displayTag(interaction.user),
        outcome: accepted ? "accepted" : "rejected",
        team: outcome.team,
        transcriptSaved: Boolean(transcriptText),
        transcript: transcriptText ? {
            text: transcriptText,
            createdAt: closedAt,
            lineCount: transcriptLines
        } : null,
        transcriptPreview: transcriptText.slice(0, 10000),
        applicantThreadImages,
        licenseAnalysis
    });

    let recruitmentLogId = "";
    if (accepted) {
        const logEntry = await appendRecruitmentLog({
            guildId: ticket.guildId,
            channelId: ticket.channelId,
            threadId: ticket.threadId,
            applicantId: ticket.applicantId,
            applicantTag: ticket.applicantTag,
            closedById: interaction.user.id,
            closedByTag: displayTag(interaction.user),
            outcome: "accepted",
            team: outcome.team,
            closedAt,
            createdAt: ticket.createdAt,
            licenseAttachments: ticket.licenseAttachments || [],
            eventAttachments: ticket.eventAttachments || [],
            applicantThreadImages,
            licenseAnalysis,
            transcriptSaved: Boolean(transcriptText),
            transcriptLineCount: transcriptLines,
            transcriptPreview: transcriptText.slice(0, 10000)
        });
        recruitmentLogId = logEntry.id;

        await sendRecruitmentLog(interaction.client, config, updatedTicket || ticket, applicantThreadImages, licenseAnalysis).catch(error => {
            console.error("Failed to send recruitment images:", error);
        });
    }

    await logAction(interaction.client, {
        type: "ticket",
        title: accepted ? "Recruitment Ticket Accepted" : "Recruitment Ticket Rejected",
        message: accepted
            ? `<@${ticket.applicantId}> was recruited to **${outcome.team}**. IGN: **${licenseAnalysis.inGameName || "not detected"}**. Previous team: **${licenseAnalysis.sourceTeam || "not detected"}**.`
            : `<@${ticket.applicantId}> was rejected. IGN: **${licenseAnalysis.inGameName || "not detected"}**. Previous team: **${licenseAnalysis.sourceTeam || "not detected"}**.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: {
            threadId: thread.id,
            outcome: accepted ? "accepted" : "rejected",
            team: outcome.team,
            recruitmentLogId,
            licenseAnalysis
        }
    });

    if (accepted && config.memberCounts?.updateOnRecruitmentClose) {
        await incrementTeamCount(interaction.client, outcome.team, 1, interaction.user).catch(error => {
            console.error("Failed to update member count after recruitment close:", error.message);
        });
    }

    if (accepted) {
        await queueTeamRoleAssignment(interaction.client, {
            userId: ticket.applicantId,
            teamName: outcome.team,
            ticketId: ticket.threadId,
            actor: interaction.user
        }).catch(error => {
            console.error("Failed to queue team role assignment:", error.message);
        });
    }

    const outcomeText = accepted ? `recruited to **${outcome.team}**` : "rejected";
    await thread.send(`Ticket closed by <@${interaction.user.id}>. Applicant was ${outcomeText}.`);

    await acknowledgeCloseSelection(interaction, {
        content: `Ticket closed. Outcome: ${accepted ? outcome.team : "Rejected"}.`,
        components: []
    });

    await thread.setLocked(true, "Recruitment ticket closed").catch(() => null);
    await thread.setArchived(true, "Recruitment ticket closed").catch(() => null);
}

async function addUsersToTicket(interaction, users) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    const uniqueUsers = [...new Map(users.filter(Boolean).map(user => [user.id, user])).values()].slice(0, 25);
    if (!uniqueUsers.length) {
        await respondEphemeral(interaction, "No valid users were provided.");
        return;
    }

    const added = [];
    const failed = [];
    for (const user of uniqueUsers) {
        try {
            await thread.members.add(user.id);
            added.push(user.id);
        } catch (error) {
            failed.push(`${displayTag(user)} (${error.message})`);
        }
    }

    const nextAdded = [...new Set([...(ticket.addedUserIds || []), ...added])];
    await updateTicket(thread.id, { addedUserIds: nextAdded });

    if (added.length) {
        await thread.send(`Added ${added.map(id => `<@${id}>`).join(", ")} to the ticket.`);
        await logAction(interaction.client, {
            type: "ticket",
            title: "Users Added To Recruitment Ticket",
            message: `<@${interaction.user.id}> added ${added.length} user(s) to a ticket.`,
            guildId: ticket.guildId,
            actorId: interaction.user.id,
            actorTag: displayTag(interaction.user),
            targetId: ticket.applicantId,
            targetTag: ticket.applicantTag,
            metadata: { threadId: thread.id, addedUserIds: added }
        });
    }

    await respondEphemeral(interaction, [
        added.length ? `Added: ${added.map(id => `<@${id}>`).join(", ")}` : "No users were added.",
        failed.length ? `Failed: ${failed.join("; ")}` : ""
    ].filter(Boolean).join("\n"));
}

async function addUserToTicket(interaction, user) {
    await addUsersToTicket(interaction, [user]);
}

async function massAddUsersToTicket(interaction, rawValue) {
    const ids = [...new Set(String(rawValue || "").match(/\d{10,25}/g) || [])].slice(0, 25);
    const users = [];

    for (const id of ids) {
        const user = await interaction.client.users.fetch(id).catch(() => null);
        if (user) users.push(user);
    }

    await addUsersToTicket(interaction, users);
}

async function removeUserFromTicket(interaction, user) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    await thread.members.remove(user.id).catch(() => null);
    await updateTicket(thread.id, {
        addedUserIds: (ticket.addedUserIds || []).filter(id => id !== user.id)
    });

    await thread.send(`Removed <@${user.id}> from the ticket.`);
    await logAction(interaction.client, {
        type: "ticket",
        title: "User Removed From Recruitment Ticket",
        message: `<@${interaction.user.id}> removed <@${user.id}> from a ticket.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: user.id,
        targetTag: displayTag(user),
        metadata: { threadId: thread.id, applicantId: ticket.applicantId }
    });

    await respondEphemeral(interaction, `Removed <@${user.id}> from this ticket.`);
}

async function renameTicket(interaction, rawName) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    const name = safeTicketName(rawName);
    if (!name) {
        await respondEphemeral(interaction, "Please provide a valid thread name.");
        return;
    }

    await thread.setName(name, `Recruitment ticket renamed by ${displayTag(interaction.user)}`);
    await updateTicket(thread.id, { renamedAt: new Date().toISOString(), renamedById: interaction.user.id });
    await thread.send(`Ticket renamed to **${name}** by <@${interaction.user.id}>.`);

    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Ticket Renamed",
        message: `<@${interaction.user.id}> renamed a ticket to **${name}**.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: { threadId: thread.id, name }
    });

    await respondEphemeral(interaction, `Renamed this ticket to **${name}**.`);
}

async function deleteTicket(interaction) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    await updateTicket(thread.id, {
        status: "deleted",
        deletedAt: new Date().toISOString(),
        deletedById: interaction.user.id,
        deletedByTag: displayTag(interaction.user)
    });

    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Ticket Deleted",
        message: `<@${interaction.user.id}> deleted the ticket for <@${ticket.applicantId}>.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: { threadId: thread.id }
    });

    await respondEphemeral(interaction, "Deleting this ticket thread now.");
    await thread.delete("Recruitment ticket deleted by recruiter").catch(async () => {
        await thread.setLocked(true, "Recruitment ticket deleted by recruiter").catch(() => null);
        await thread.setArchived(true, "Recruitment ticket deleted by recruiter").catch(() => null);
    });
}

async function archiveTicket(interaction) {
    if (!(await requireRecruiter(interaction))) return;

    const context = await getTicketContext(interaction);
    if (!context) return;

    const { thread, ticket } = context;
    await updateTicket(thread.id, {
        status: ticket.status === "closed" ? "closed" : "archived",
        archivedAt: new Date().toISOString(),
        archivedById: interaction.user.id,
        archivedByTag: displayTag(interaction.user)
    });

    await logAction(interaction.client, {
        type: "ticket",
        title: "Recruitment Ticket Archived",
        message: `<@${interaction.user.id}> archived the ticket for <@${ticket.applicantId}>.`,
        guildId: ticket.guildId,
        actorId: interaction.user.id,
        actorTag: displayTag(interaction.user),
        targetId: ticket.applicantId,
        targetTag: ticket.applicantTag,
        metadata: { threadId: thread.id }
    });

    await respondEphemeral(interaction, "Archiving this ticket thread now.");
    await thread.setLocked(true, "Recruitment ticket archived").catch(() => null);
    await thread.setArchived(true, "Recruitment ticket archived").catch(() => null);
}

async function handleRecruitmentInteraction(interaction) {
    if (!interaction.isButton() && !interaction.isModalSubmit()) {
        return false;
    }

    if (!interaction.customId.startsWith("recruitment:")) {
        return false;
    }

    const customId = interaction.customId;

    try {
        const config = await loadDashboardConfig();

        if (!config.recruitment.enabled && customId === APPLY_BUTTON_ID) {
            await interaction.reply({ content: "Recruitment is currently closed.", ephemeral: true });
            return true;
        }

        if (customId === APPLY_BUTTON_ID) {
            await collectLicense(interaction);
        } else if (customId.startsWith(EVENT_YES_PREFIX)) {
            await collectEventScreenshots(interaction);
        } else if (customId.startsWith(EVENT_NO_PREFIX)) {
            await completeWithoutEvents(interaction);
        } else if (customId === CLAIM_ID) {
            await claimTicket(interaction);
        } else if (customId === CLOSE_ID) {
            await startClose(interaction);
        } else if (customId.startsWith(TUTORIAL_PREFIX)) {
            await sendTutorial(interaction, customId.slice(TUTORIAL_PREFIX.length));
        } else if (customId.startsWith(CLOSE_TEAM_PREFIX)) {
            await finishClose(interaction, customId.slice(CLOSE_TEAM_PREFIX.length));
        }
    } catch (error) {
        console.error("Recruitment interaction failed:", error);
        const payload = { content: `Recruitment action failed: ${error.message}`, ephemeral: Boolean(interaction.guildId) };

        if (interaction.deferred || interaction.replied) {
            await interaction.followUp(payload).catch(() => null);
        } else {
            await interaction.reply(payload).catch(() => null);
        }
    }

    return true;
}

module.exports = {
    APPLY_BUTTON_ID,
    RECRUITMENT_OUTCOMES,
    addTicketScreenshots,
    addUserToTicket,
    archiveTicket,
    buildPanelPayload,
    changeTicketScreenshot,
    claimTicket,
    deleteTicket,
    ensureRecruitmentPanel,
    finishClose,
    handleRecruitmentInteraction,
    listTicketScreenshots,
    massAddUsersToTicket,
    memberCanRecruit,
    removeTicketScreenshot,
    removeUserFromTicket,
    renameTicket,
    sendInviteToApplicant,
    sendTutorial,
    startClose
};
