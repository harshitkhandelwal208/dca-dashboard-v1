const crypto = require("crypto");
const { filePathFor, readState, writeState } = require("./stateStore");

const CONFIG_SCOPE = "dashboardConfig";
const CONFIG_PATH = filePathFor(CONFIG_SCOPE);
const SNOWFLAKE_RE = /^\d{10,25}$/;
const HEX_COLOR_RE = /^#?[0-9a-f]{6}$/i;
const CONFIG_VERSION = 9;

const DEFAULT_RECRUITMENT_QUESTIONS = [
    "Which team do you want to join?",
    "Do you have other accounts, if yes, in which team?",
    "Were you ever flagged/banned, had double VIP?",
    "Do you want to stay long-term or just visit?",
    "How many km's do you usually drive per week?"
].join("\n");

const RECRUITMENT_TEAMS = [
    "Discord",
    "Discord\u00b2",
    "Discord 3\u2122",
    "Nascar DC"
];

const DEFAULT_YOUTUBE_FEEDS = [
    { id: "UCyL-QGEkA1r7R7U5rN_Yonw", name: "Vereshchak", channelId: "1341719063780393031", enabled: true, lastVideoId: "" },
    { id: "UC16xML3oyIZDeF3g8nnV6MA", name: "Vokope", channelId: "1341719063780393031", enabled: true, lastVideoId: "" },
    { id: "UCBrnPp4lpRukfuvXUiRz6_A", name: "Soulis HCR2", channelId: "1341719134135779389", enabled: true, lastVideoId: "" },
    { id: "UCwxuNdbZ-nK5oUEeY1tY9CQ", name: "tas HCR2", channelId: "1341719134135779389", enabled: true, lastVideoId: "" },
    { id: "UCBHmJJ0PN-efNW5PFdJ4EDQ", name: "PROJECT GER", channelId: "1341719134135779389", enabled: true, lastVideoId: "" },
    { id: "UCv_5HRU2ctFoYNeWFGLNoXw", name: "Exodus Hcr2", channelId: "1341719134135779389", enabled: true, lastVideoId: "" },
    { id: "UCF0iJo2klF-QGxzDDmOkQbQ", name: "Zorro HCR2", channelId: "1341719134135779389", enabled: true, lastVideoId: "" },
    { id: "UCnCaLcVf4YsPcsvi6PE4m6A", name: "ChillHcr2Guy", channelId: "1341733821707452437", enabled: true, lastVideoId: "" }
];

const DEFAULT_MEMBER_TEAMS = [
    { id: "discord", name: "Discord", division: "Division-CC", players: 43, recruitmentStatus: "Closed", aliases: ["Discord"] },
    { id: "discord2", name: "Discord\u00b2", division: "Division-CC", players: 49, recruitmentStatus: "Closed", aliases: ["Discord\u00b2", "Discord2"] },
    { id: "discord3", name: "Discord 3\u2122", division: "Division-I", players: 49, recruitmentStatus: "Closed", aliases: ["Discord 3\u2122", "Discord3"] },
    { id: "nascar-dc", name: "Nascar DC", division: "Division-II", players: 0, recruitmentStatus: "Open", aliases: ["Nascar DC"] },
    { id: "baja-dc", name: "Baja DC", division: "Division-II", players: 50, recruitmentStatus: "Open", aliases: ["Baja DC"] },
    { id: "formula-dcx", name: "Formula DCx", division: "Division-VI", players: 50, recruitmentStatus: "Open", aliases: ["Formula DCx"] },
    { id: "rally-dcy", name: "Rally DCy", division: "Division-IV", players: 49, recruitmentStatus: "Closed", aliases: ["Rally DCy"] }
];

const DEFAULT_SPREADSHEET_TEAMS = DEFAULT_MEMBER_TEAMS.map(team => ({
    id: team.id,
    name: team.name,
    enabled: false,
    monitoredChannelId: "",
    outputChannelId: "",
    accessRoleId: "",
    ownTeamAliases: [...new Set([team.name, ...(team.aliases || [])])],
    ownPlayerAliases: [],
    autoProcess: true
}));

const DEFAULT_CONFIG = {
    version: CONFIG_VERSION,
    updatedAt: new Date(0).toISOString(),
    bot: {
        guildId: "",
        communityGuildId: "",
        recruitmentGuildId: "",
        dashboardAllowedRoleId: "",
        recruiterRoleId: "",
        managerRoleId: "",
        locale: "en-US",
        commandLogChannelId: "",
        dashboardUrl: ""
    },
    logging: {
        enabled: true,
        channelId: "",
        events: {
            tickets: true,
            memberCounts: true,
            youtube: true,
            reactionRoles: true,
            system: true
        }
    },
    welcome: {
        enabled: true,
        channelId: "916042813425201152",
        message: [
            "Hey {member}! Welcome to the **Discord Alliance server**!",
            "",
            "> 1. **Head to <#839605609027600415>**",
            "> Read the server rules carefully, and once done, press the confirmation reaction to get access.",
            ">",
            "> 2. **Unlock More Channels**",
            "> Go to <#840310137390104627> and select your desired option to access more channels of this server.",
            ">",
            "> 3. **Name Policy**",
            "> Please make sure your in-game name and your Discord display name matches in this server.",
            "> This helps leaders identify you easily.",
            "",
            "**Have fun and enjoy your time here!**"
        ].join("\n")
    },
    leave: {
        enabled: true,
        channelId: "839905184154517597",
        message: "{member} has left the server."
    },
    recruitment: {
        enabled: true,
        panelChannelId: "",
        panelMessageId: "",
        panelTitle: "DCA Team Recruitment",
        panelDescription: "Ready to apply for the DCA teams? Press **Apply!** and follow the private prompts.",
        panelColor: "#0f766e",
        logChannelId: "",
        recruiterAlertChannelId: "",
        tutorialUploadChannelId: "",
        screenshotDmUserId: "",
        recruiterRoleId: "",
        inviteGuildId: "",
        inviteChannelId: "",
        inviteMessage: "{user} Your application was accepted. Join **{server}** here: {invite}",
        communityRulesRoleId: "",
        banListChannelId: "",
        banListMessageIds: [],
        privateThreads: true,
        threadAutoArchiveMinutes: 10080,
        maxOpenTicketsPerUser: 1,
        transcriptOnClose: true,
        deleteOnClose: false,
        questionsIntro: "Great that you want to join us! Please answer the following questions so that we can find out what team fits you the best.",
        questions: DEFAULT_RECRUITMENT_QUESTIONS,
        teams: RECRUITMENT_TEAMS,
        tutorials: [
            {
                id: "license-screenshot",
                label: "License Screenshot",
                description: "How to upload an uncropped driver's license screenshot with coins and gems visible.",
                videoUrl: "",
                enabled: true
            },
            {
                id: "player-stats",
                label: "Player Stats",
                description: "How to show your player stats for recruiters.",
                videoUrl: "",
                enabled: true
            }
        ]
    },
    memberCounts: {
        enabled: true,
        channelId: "1341563215611433035",
        messageId: "",
        title: "Member Count",
        updateOnRecruitmentClose: true,
        teams: DEFAULT_MEMBER_TEAMS
    },
    spreadsheets: {
        enabled: false,
        sessionWindowMinutes: 1,
        outputFormat: "xlsx",
        libreOfficePath: "",
        geminiModel: "gemini-2.5-flash",
        geminiTimeoutMs: 300000,
        geminiMaxRetries: 4,
        rawDataRetentionDays: 31,
        imageRetentionDays: 7,
        teams: DEFAULT_SPREADSHEET_TEAMS
    },
    youtube: {
        enabled: true,
        checkIntervalMinutes: 5,
        maxAnnouncementAgeHours: 72,
        defaultChannelId: "",
        announcementTemplate: "**{name}** uploaded a new video!\n{url}",
        feeds: DEFAULT_YOUTUBE_FEEDS
    },
    reactionRoles: [
        {
            id: "language-unlock",
            name: "Other languages",
            enabled: true,
            channelId: "840310137390104627",
            messageId: "",
            message: "If you want to speak in other language choose the confirmation reaction to select that.",
            options: [{ emoji: "\u2611\uFE0F", roleId: "842089922768797726", label: "Other language" }]
        },
        {
            id: "event-pings",
            name: "Organized event pings",
            enabled: true,
            channelId: "839907517663936612",
            messageId: "",
            message: "React with thumbsup if you want ping everytime there is a organized event.",
            options: [{ emoji: "\uD83D\uDC4D", roleId: "840250757235212339", label: "PE call" }]
        }
    ]
};

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function shortId(prefix = "item") {
    return `${prefix}-${crypto.randomBytes(6).toString("hex")}`;
}

function isSnowflake(value) {
    return typeof value === "string" && SNOWFLAKE_RE.test(value.trim());
}

function cleanSnowflake(value, fallback = "") {
    if (isSnowflake(value)) return value.trim();
    return fallback;
}

function cleanBoolean(value, fallback) {
    if (typeof value === "boolean") return value;
    return Boolean(fallback);
}

function cleanText(value, fallback, maxLength) {
    if (typeof value !== "string") return fallback;
    const text = value.trim().length ? value : fallback;
    return text.slice(0, maxLength);
}

function cleanOptionalText(value, fallback, maxLength) {
    if (typeof value !== "string") return fallback;
    return value.slice(0, maxLength);
}

function cleanName(value, fallback, maxLength = 80) {
    if (typeof value !== "string") return fallback;
    const text = value.trim();
    return (text || fallback).slice(0, maxLength);
}

function cleanId(value, fallback, prefix = "item") {
    if (typeof value === "string" && /^[a-zA-Z0-9_-]{1,80}$/.test(value)) return value;
    return fallback || shortId(prefix);
}

function cleanColor(value, fallback) {
    if (typeof value !== "string") return fallback;
    const text = value.trim();
    if (!HEX_COLOR_RE.test(text)) return fallback;
    return text.startsWith("#") ? text : `#${text}`;
}

function cleanNumber(value, fallback, min, max) {
    const number = Number(value);
    if (!Number.isFinite(number)) return fallback;
    return Math.min(max, Math.max(min, Math.round(number)));
}

function cleanStringList(value, fallback = [], maxItems = 20, maxLength = 80) {
    const raw = Array.isArray(value)
        ? value
        : typeof value === "string"
            ? value.split(/[\n,]+/)
            : Array.isArray(fallback)
                ? fallback
                : [];

    return [...new Set(raw
        .map(item => cleanName(String(item || ""), "", maxLength))
        .filter(Boolean))]
        .slice(0, maxItems);
}

function normalizeBot(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};

    return {
        guildId: cleanSnowflake(raw.guildId, process.env.DISCORD_GUILD_ID || fallback.guildId || ""),
        communityGuildId: cleanSnowflake(raw.communityGuildId, raw.guildId || process.env.COMMUNITY_GUILD_ID || fallback.communityGuildId || fallback.guildId || ""),
        recruitmentGuildId: cleanSnowflake(raw.recruitmentGuildId, process.env.RECRUITMENT_GUILD_ID || fallback.recruitmentGuildId || fallback.guildId || ""),
        dashboardAllowedRoleId: cleanSnowflake(
            raw.dashboardAllowedRoleId,
            process.env.DASHBOARD_ALLOWED_ROLE_ID || process.env.DISCORD_DASHBOARD_ROLE_ID || fallback.dashboardAllowedRoleId || ""
        ),
        recruiterRoleId: cleanSnowflake(raw.recruiterRoleId, process.env.RECRUITER_ROLE_ID || fallback.recruiterRoleId || ""),
        managerRoleId: cleanSnowflake(raw.managerRoleId, fallback.managerRoleId || ""),
        locale: cleanName(raw.locale, fallback.locale || "en-US", 20),
        commandLogChannelId: cleanSnowflake(raw.commandLogChannelId, fallback.commandLogChannelId || ""),
        dashboardUrl: cleanOptionalText(raw.dashboardUrl, process.env.DASHBOARD_BASE_URL || fallback.dashboardUrl || "", 300)
    };
}

function normalizeLogging(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};
    const rawEvents = raw.events && typeof raw.events === "object" ? raw.events : {};

    return {
        enabled: cleanBoolean(raw.enabled, fallback.enabled),
        channelId: cleanSnowflake(raw.channelId, fallback.channelId || ""),
        events: Object.fromEntries(
            Object.entries(fallback.events).map(([key, value]) => [key, cleanBoolean(rawEvents[key], value)])
        )
    };
}

function normalizeMessageSection(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};

    return {
        enabled: cleanBoolean(raw.enabled, fallback.enabled),
        channelId: cleanSnowflake(raw.channelId, fallback.channelId),
        message: cleanText(raw.message, fallback.message, 2000)
    };
}

function normalizeReactionOption(input) {
    const raw = input && typeof input === "object" ? input : {};
    const emoji = cleanName(raw.emoji, "", 128);
    const roleId = cleanSnowflake(raw.roleId, "");

    if (!emoji || !roleId) return null;

    return {
        emoji,
        roleId,
        label: cleanName(raw.label, "", 80)
    };
}

function normalizeReactionGroup(input, fallback, index) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : {};
    const options = Array.isArray(raw.options) ? raw.options : base.options;

    return {
        id: cleanId(raw.id, base.id || `reaction-${index + 1}`, "reaction"),
        name: cleanName(raw.name, base.name || `Reaction message ${index + 1}`),
        enabled: cleanBoolean(raw.enabled, base.enabled !== false),
        channelId: cleanSnowflake(raw.channelId, base.channelId || ""),
        messageId: cleanSnowflake(raw.messageId, base.messageId || ""),
        message: cleanText(raw.message, base.message || "React below to choose a role.", 2000),
        options: (Array.isArray(options) ? options : [])
            .slice(0, 25)
            .map(normalizeReactionOption)
            .filter(Boolean)
    };
}

function normalizeTutorial(input, fallback, index) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : {};

    return {
        id: cleanId(raw.id, base.id || `tutorial-${index + 1}`, "tutorial"),
        label: cleanName(raw.label, base.label || `Tutorial ${index + 1}`, 40),
        description: cleanOptionalText(raw.description, base.description || "", 500),
        videoUrl: cleanOptionalText(raw.videoUrl, base.videoUrl || "", 1000),
        enabled: cleanBoolean(raw.enabled, base.enabled !== false)
    };
}

function normalizeRecruitment(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : DEFAULT_CONFIG.recruitment;
    const rawTutorials = Array.isArray(raw.tutorials) ? raw.tutorials : base.tutorials;
    const rawTeams = Array.isArray(raw.teams) ? raw.teams : base.teams;

    return {
        enabled: cleanBoolean(raw.enabled, base.enabled !== false),
        panelChannelId: cleanSnowflake(raw.panelChannelId, base.panelChannelId || ""),
        panelMessageId: cleanSnowflake(raw.panelMessageId, base.panelMessageId || ""),
        panelTitle: cleanName(raw.panelTitle, base.panelTitle, 120),
        panelDescription: cleanText(raw.panelDescription, base.panelDescription, 2000),
        panelColor: cleanColor(raw.panelColor, base.panelColor || "#0f766e"),
        logChannelId: cleanSnowflake(raw.logChannelId, base.logChannelId || ""),
        recruiterAlertChannelId: cleanSnowflake(raw.recruiterAlertChannelId, base.recruiterAlertChannelId || ""),
        tutorialUploadChannelId: cleanSnowflake(raw.tutorialUploadChannelId, base.tutorialUploadChannelId || ""),
        screenshotDmUserId: cleanSnowflake(raw.screenshotDmUserId, process.env.RECRUITMENT_SCREENSHOT_DM_USER_ID || base.screenshotDmUserId || ""),
        recruiterRoleId: cleanSnowflake(raw.recruiterRoleId, process.env.RECRUITER_ROLE_ID || base.recruiterRoleId || ""),
        inviteGuildId: cleanSnowflake(raw.inviteGuildId, base.inviteGuildId || ""),
        inviteChannelId: cleanSnowflake(raw.inviteChannelId, base.inviteChannelId || ""),
        inviteMessage: cleanText(
            raw.inviteMessage,
            base.inviteMessage || "{user} Your application was accepted. Join **{server}** here: {invite}",
            1200
        ),
        communityRulesRoleId: cleanSnowflake(raw.communityRulesRoleId, base.communityRulesRoleId || ""),
        banListChannelId: cleanSnowflake(raw.banListChannelId, base.banListChannelId || ""),
        banListMessageIds: (Array.isArray(raw.banListMessageIds) ? raw.banListMessageIds : base.banListMessageIds || [])
            .slice(0, 20)
            .map(value => cleanSnowflake(value, ""))
            .filter(Boolean),
        privateThreads: cleanBoolean(raw.privateThreads, base.privateThreads !== false),
        threadAutoArchiveMinutes: cleanNumber(raw.threadAutoArchiveMinutes, base.threadAutoArchiveMinutes || 10080, 60, 10080),
        maxOpenTicketsPerUser: cleanNumber(raw.maxOpenTicketsPerUser, base.maxOpenTicketsPerUser || 1, 1, 10),
        transcriptOnClose: cleanBoolean(raw.transcriptOnClose, base.transcriptOnClose !== false),
        deleteOnClose: false,
        questionsIntro: cleanText(raw.questionsIntro, base.questionsIntro, 800),
        questions: cleanText(raw.questions, base.questions || DEFAULT_RECRUITMENT_QUESTIONS, 2000),
        teams: rawTeams.slice(0, 12).map(team => cleanName(team, "", 40)).filter(Boolean),
        tutorials: rawTutorials.slice(0, 10).map((tutorial, index) => normalizeTutorial(tutorial, base.tutorials?.[index], index))
    };
}

function normalizeMemberTeam(input, fallback, index) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : {};
    const name = cleanName(raw.name, base.name || `Team ${index + 1}`, 60);
    const aliases = Array.isArray(raw.aliases) ? raw.aliases : base.aliases;
    const communityRoleId = cleanSnowflake(raw.communityRoleId, raw.roleId || base.communityRoleId || base.roleId || "");
    const communityAutoEnabled = cleanBoolean(
        raw.communityRoleAutoAssignEnabled,
        raw.autoAssignEnabled ?? base.communityRoleAutoAssignEnabled ?? base.autoAssignEnabled === true
    );
    const communityDelayMinutes = cleanNumber(
        raw.communityRoleDelayMinutes,
        raw.autoAssignDelayMinutes ?? base.communityRoleDelayMinutes ?? base.autoAssignDelayMinutes ?? 0,
        0,
        43200
    );

    return {
        id: cleanId(raw.id, base.id || name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), "team"),
        name,
        division: cleanOptionalText(raw.division, base.division || "", 80),
        players: cleanNumber(raw.players, base.players || 0, 0, 999),
        recruitmentStatus: cleanName(raw.recruitmentStatus, base.recruitmentStatus || "Open", 40),
        recruitmentRoleId: cleanSnowflake(raw.recruitmentRoleId, base.recruitmentRoleId || ""),
        recruitmentRoleAutoAssignEnabled: cleanBoolean(raw.recruitmentRoleAutoAssignEnabled, base.recruitmentRoleAutoAssignEnabled === true),
        recruitmentRoleDelayMinutes: cleanNumber(raw.recruitmentRoleDelayMinutes, base.recruitmentRoleDelayMinutes || 0, 0, 43200),
        communityRoleId,
        communityRoleAutoAssignEnabled: communityAutoEnabled,
        communityRoleDelayMinutes: communityDelayMinutes,
        roleId: communityRoleId,
        autoAssignEnabled: communityAutoEnabled,
        autoAssignDelayMinutes: communityDelayMinutes,
        aliases: (Array.isArray(aliases) ? aliases : [])
            .slice(0, 10)
            .map(alias => cleanName(alias, "", 60))
            .filter(Boolean)
    };
}

function normalizeMemberCounts(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : DEFAULT_CONFIG.memberCounts;
    const teams = Array.isArray(raw.teams) ? raw.teams : base.teams;

    return {
        enabled: cleanBoolean(raw.enabled, base.enabled !== false),
        channelId: cleanSnowflake(raw.channelId, base.channelId || ""),
        messageId: cleanSnowflake(raw.messageId, base.messageId || ""),
        title: cleanName(raw.title, base.title || "Member Count", 100),
        updateOnRecruitmentClose: cleanBoolean(raw.updateOnRecruitmentClose, base.updateOnRecruitmentClose !== false),
        teams: teams.slice(0, 25).map((team, index) => normalizeMemberTeam(team, base.teams?.[index], index))
    };
}

function normalizeSpreadsheetTeam(input, fallback, index) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : {};
    const name = cleanName(raw.name, base.name || `Team ${index + 1}`, 60);

    return {
        id: cleanId(raw.id, base.id || name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), "spreadsheet-team"),
        name,
        enabled: cleanBoolean(raw.enabled, base.enabled === true),
        monitoredChannelId: cleanSnowflake(raw.monitoredChannelId, base.monitoredChannelId || ""),
        outputChannelId: cleanSnowflake(raw.outputChannelId, base.outputChannelId || ""),
        accessRoleId: cleanSnowflake(raw.accessRoleId, base.accessRoleId || ""),
        ownTeamAliases: cleanStringList(raw.ownTeamAliases, base.ownTeamAliases || [name], 25, 80),
        ownPlayerAliases: cleanStringList(raw.ownPlayerAliases, base.ownPlayerAliases || [], 250, 80),
        autoProcess: cleanBoolean(raw.autoProcess, base.autoProcess !== false)
    };
}

function normalizeSpreadsheets(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : DEFAULT_CONFIG.spreadsheets;
    const rawTeams = Array.isArray(raw.teams) ? raw.teams : base.teams;
    const outputFormat = cleanName(raw.outputFormat, base.outputFormat || "xlsx", 10).toLowerCase();

    return {
        enabled: cleanBoolean(raw.enabled, base.enabled === true),
        sessionWindowMinutes: cleanNumber(raw.sessionWindowMinutes, base.sessionWindowMinutes || 1, 1, 30),
        outputFormat: ["xlsx", "fods"].includes(outputFormat) ? outputFormat : "xlsx",
        libreOfficePath: cleanOptionalText(raw.libreOfficePath, process.env.LIBREOFFICE_PATH || base.libreOfficePath || "", 500),
        geminiModel: cleanName(raw.geminiModel, process.env.GEMINI_FLASH_MODEL || process.env.GEMINI_MODEL || base.geminiModel || "gemini-2.5-flash", 80),
        geminiTimeoutMs: cleanNumber(raw.geminiTimeoutMs, Number(process.env.GEMINI_TIMEOUT_MS || base.geminiTimeoutMs || 300000), 30000, 900000),
        geminiMaxRetries: cleanNumber(raw.geminiMaxRetries, Number(process.env.GEMINI_MAX_RETRIES || base.geminiMaxRetries || 4), 0, 10),
        rawDataRetentionDays: cleanNumber(raw.rawDataRetentionDays, base.rawDataRetentionDays || 31, 1, 370),
        imageRetentionDays: cleanNumber(raw.imageRetentionDays, Number(process.env.SPREADSHEET_IMAGE_RETENTION_DAYS || base.imageRetentionDays || 7), 1, 90),
        teams: (Array.isArray(rawTeams) ? rawTeams : [])
            .slice(0, 30)
            .map((team, index) => normalizeSpreadsheetTeam(team, base.teams?.[index], index))
    };
}

function normalizeYoutubeFeed(input, fallback, index) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : {};
    const sentVideoIds = cleanStringList(
        [
            ...(Array.isArray(raw.sentVideoIds) ? raw.sentVideoIds : []),
            ...(Array.isArray(raw.postedVideoIds) ? raw.postedVideoIds : []),
            ...(Array.isArray(raw.seenVideoIds) ? raw.seenVideoIds : []),
            ...(Array.isArray(base.sentVideoIds) ? base.sentVideoIds : []),
            raw.lastVideoId,
            base.lastVideoId
        ],
        [],
        50,
        80
    );

    return {
        id: cleanName(raw.id, base.id || "", 120),
        name: cleanName(raw.name, base.name || `Feed ${index + 1}`, 80),
        channelId: cleanSnowflake(raw.channelId, base.channelId || ""),
        enabled: cleanBoolean(raw.enabled, base.enabled !== false),
        lastVideoId: cleanOptionalText(raw.lastVideoId, base.lastVideoId || sentVideoIds[0] || "", 80),
        lastPublishedAt: cleanOptionalText(raw.lastPublishedAt, base.lastPublishedAt || "", 80),
        lastCheckedAt: cleanOptionalText(raw.lastCheckedAt, base.lastCheckedAt || "", 80),
        sentVideoIds
    };
}

function normalizeYoutube(input, fallback) {
    const raw = input && typeof input === "object" ? input : {};
    const base = fallback && typeof fallback === "object" ? fallback : DEFAULT_CONFIG.youtube;
    const feeds = Array.isArray(raw.feeds) ? raw.feeds : base.feeds;

    return {
        enabled: cleanBoolean(raw.enabled, base.enabled !== false),
        checkIntervalMinutes: cleanNumber(raw.checkIntervalMinutes, base.checkIntervalMinutes || 5, 1, 1440),
        maxAnnouncementAgeHours: cleanNumber(raw.maxAnnouncementAgeHours, base.maxAnnouncementAgeHours || 72, 1, 720),
        defaultChannelId: cleanSnowflake(raw.defaultChannelId, base.defaultChannelId || ""),
        announcementTemplate: cleanText(raw.announcementTemplate, base.announcementTemplate, 1200),
        feeds: feeds.slice(0, 50).map((feed, index) => normalizeYoutubeFeed(feed, base.feeds?.[index], index)).filter(feed => feed.id)
    };
}

function normalizeDashboardConfig(input, options = {}) {
    const raw = input && typeof input === "object" ? input : {};
    const defaults = clone(DEFAULT_CONFIG);
    const fallbackRoles = Array.isArray(raw.reactionRoles) ? raw.reactionRoles : defaults.reactionRoles;
    const spreadsheetsInput = Number(raw.version || 0) < 7 && raw.spreadsheets?.sessionWindowMinutes === 5
        ? { ...raw.spreadsheets, sessionWindowMinutes: 1 }
        : raw.spreadsheets;

    return {
        version: CONFIG_VERSION,
        updatedAt: options.preserveUpdatedAt && typeof raw.updatedAt === "string" ? raw.updatedAt : new Date().toISOString(),
        bot: normalizeBot(raw.bot, defaults.bot),
        logging: normalizeLogging(raw.logging, defaults.logging),
        welcome: normalizeMessageSection(raw.welcome, defaults.welcome),
        leave: normalizeMessageSection(raw.leave, defaults.leave),
        recruitment: normalizeRecruitment(raw.recruitment, defaults.recruitment),
        memberCounts: normalizeMemberCounts(raw.memberCounts, defaults.memberCounts),
        spreadsheets: normalizeSpreadsheets(spreadsheetsInput, defaults.spreadsheets),
        youtube: normalizeYoutube(raw.youtube, defaults.youtube),
        reactionRoles: fallbackRoles.slice(0, 25).map((group, index) => normalizeReactionGroup(group, defaults.reactionRoles[index], index))
    };
}

async function loadDashboardConfig() {
    const raw = await readState(CONFIG_SCOPE, DEFAULT_CONFIG);
    return normalizeDashboardConfig(raw, { preserveUpdatedAt: true });
}

async function saveDashboardConfig(config) {
    const normalized = normalizeDashboardConfig(config);
    await writeState(CONFIG_SCOPE, normalized);
    return normalized;
}

module.exports = {
    CONFIG_PATH,
    DEFAULT_CONFIG,
    DEFAULT_RECRUITMENT_QUESTIONS,
    DEFAULT_YOUTUBE_FEEDS,
    DEFAULT_MEMBER_TEAMS,
    DEFAULT_SPREADSHEET_TEAMS,
    RECRUITMENT_TEAMS,
    loadDashboardConfig,
    saveDashboardConfig,
    normalizeDashboardConfig,
    isSnowflake
};
