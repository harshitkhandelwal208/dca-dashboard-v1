const fs = require("fs");
const path = require("path");
const express = require("express");
const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { listLogs } = require("./logStore");
const { getTicket, listRecruitmentLogs, listTickets, updateTicket } = require("./recruitmentStore");
const {
    discordBotRequest,
    ensureRecruitmentPanel,
    getBotUser,
    getGuildLookups,
    syncRecruitmentBanList,
    syncMemberCountMessage,
    syncReactionRoles
} = require("./discordApi");
const {
    getOAuthConfig,
    getSession,
    registerDashboardAuthRoutes,
    requireDashboardAuth
} = require("./dashboardAuth");

const DIST_DIR = path.join(__dirname, "..", "dist");
const PUBLIC_DIR = fs.existsSync(DIST_DIR) ? DIST_DIR : path.join(__dirname, "..", "public");
const UPLOAD_DIR = process.env.DASHBOARD_UPLOAD_DIR
    ? path.resolve(process.env.DASHBOARD_UPLOAD_DIR)
    : path.join(__dirname, "..", "uploads");
const rawVideoUpload = express.raw({
    type: "*/*",
    limit: process.env.DASHBOARD_UPLOAD_LIMIT || "100mb"
});

function avatarUrl(user) {
    if (!user?.avatar) return "";
    return `https://cdn.discordapp.com/avatars/${user.id}/${user.avatar}.png?size=80`;
}

function getBaseUrl(req) {
    if (process.env.DASHBOARD_PUBLIC_URL) return process.env.DASHBOARD_PUBLIC_URL.replace(/\/$/, "");
    if (process.env.DASHBOARD_BASE_URL) return process.env.DASHBOARD_BASE_URL.replace(/\/$/, "");

    const protocol = req.headers["x-forwarded-proto"] || req.protocol || "http";
    return `${String(protocol).split(",")[0]}://${req.get("host")}`;
}

function sanitizeFilename(value) {
    return String(value || "video")
        .toLowerCase()
        .replace(/[^a-z0-9._-]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 80) || "video";
}

function extensionForUpload(req) {
    const headerName = String(req.headers["x-file-name"] || "");
    const fromName = path.extname(headerName).toLowerCase();
    if (/^\.(mp4|mov|webm|m4v)$/i.test(fromName)) return fromName;

    const type = String(req.headers["content-type"] || "").toLowerCase();
    if (type.includes("quicktime")) return ".mov";
    if (type.includes("webm")) return ".webm";
    if (type.includes("mp4")) return ".mp4";
    return ".mp4";
}

async function fetchDiscordTranscript(threadId, limit = 250) {
    const collected = [];
    let before = "";

    while (collected.length < limit) {
        const query = new URLSearchParams({ limit: String(Math.min(100, limit - collected.length)) });
        if (before) query.set("before", before);
        const batch = await discordBotRequest(`/channels/${threadId}/messages?${query.toString()}`).catch(() => []);
        if (!Array.isArray(batch) || !batch.length) break;

        collected.push(...batch);
        before = batch[batch.length - 1]?.id || "";
        if (batch.length < 100) break;
    }

    const sorted = collected.sort((a, b) => Date.parse(a.timestamp) - Date.parse(b.timestamp));
    const lines = sorted.map(message => {
        const author = message.author?.global_name || message.author?.username || message.author?.id || "Unknown";
        const attachments = (message.attachments || []).map(item => item.url).filter(Boolean);
        const embeds = (message.embeds || []).flatMap(embed => [
            embed.title,
            embed.description,
            embed.url,
            embed.image?.url,
            ...(embed.fields || []).flatMap(field => [`${field.name}: ${field.value}`])
        ]).filter(Boolean);
        const content = String(message.content || "").replace(/\s+/g, " ").trim();
        const extras = [...attachments, ...embeds].join(" ");
        return `[${new Date(message.timestamp).toISOString()}] ${author}: ${content}${extras ? ` ${extras}` : ""}`.trim();
    });

    return {
        text: lines.join("\n").slice(0, 300000),
        createdAt: new Date().toISOString(),
        lineCount: lines.length,
        source: "discord"
    };
}

async function saveTutorialUpload(req, tutorialId, config) {
    if (!Buffer.isBuffer(req.body) || !req.body.length) {
        throw new Error("No video data was uploaded.");
    }

    const baseName = sanitizeFilename(path.basename(String(req.headers["x-file-name"] || tutorialId), path.extname(String(req.headers["x-file-name"] || ""))));
    const fileName = `${tutorialId}-${Date.now()}-${baseName}${extensionForUpload(req)}`;

    const uploadChannelId = config?.recruitment?.tutorialUploadChannelId || process.env.DASHBOARD_UPLOAD_CHANNEL_ID || "";
    if (uploadChannelId) {
        const form = new FormData();
        const blob = new Blob([req.body], {
            type: String(req.headers["content-type"] || "application/octet-stream")
        });

        form.append("payload_json", JSON.stringify({
            content: `Tutorial upload: ${tutorialId}`,
            allowed_mentions: { parse: [] }
        }));
        form.append("files[0]", blob, fileName);

        const message = await discordBotRequest(`/channels/${uploadChannelId}/messages`, {
            method: "POST",
            body: form
        });
        const attachmentUrl = message.attachments?.[0]?.url;
        if (!attachmentUrl) throw new Error("Discord did not return an attachment URL.");
        return attachmentUrl;
    }

    await fs.promises.mkdir(UPLOAD_DIR, { recursive: true });
    const filePath = path.join(UPLOAD_DIR, fileName);

    if (!filePath.startsWith(UPLOAD_DIR)) {
        throw new Error("Invalid upload path.");
    }

    await fs.promises.writeFile(filePath, req.body);
    return `${getBaseUrl(req)}/uploads/${encodeURIComponent(fileName)}`;
}

function registerDashboardRoutes(app) {
    const requireAuth = requireDashboardAuth();

    registerDashboardAuthRoutes(app);

    app.use(express.static(PUBLIC_DIR, {
        index: false,
        maxAge: "1h"
    }));

    app.use("/dashboard", express.static(PUBLIC_DIR, {
        index: false,
        maxAge: "1h"
    }));

    app.use("/uploads", express.static(UPLOAD_DIR, {
        maxAge: "7d",
        fallthrough: false
    }));

    app.get(["/dashboard", "/dashboard/"], (req, res) => {
        res.sendFile(path.join(PUBLIC_DIR, "index.html"));
    });

    app.get("/api/dashboard/me", async (req, res) => {
        const session = getSession(req);
        const oauth = await getOAuthConfig(req);
        const bot = await getBotUser().catch(() => null);

        res.json({
            authenticated: Boolean(session),
            user: session ? {
                ...session.user,
                avatarUrl: avatarUrl(session.user)
            } : null,
            setup: {
                configured: oauth.configured,
                missing: oauth.missing
            },
            bot: {
                ready: Boolean(bot),
                user: bot ? {
                    id: bot.id,
                    tag: bot.discriminator && bot.discriminator !== "0"
                        ? `${bot.username}#${bot.discriminator}`
                        : bot.username
                } : null
            },
            recruitment: {
                recruiterRoleId: process.env.RECRUITER_ROLE_ID || process.env.RECRUITMENT_RECRUITER_ROLE_ID || ""
            }
        });
    });

    app.get("/api/dashboard/config", requireAuth, async (req, res) => {
        try {
            const [config, lookups, recruitmentLogs, botLogs, tickets] = await Promise.all([
                loadDashboardConfig(),
                getGuildLookups(),
                listRecruitmentLogs(50),
                listLogs(100),
                listTickets()
            ]);

            res.json({
                config,
                lookups,
                logs: recruitmentLogs,
                recruitmentLogs,
                botLogs,
                tickets
            });
        } catch (error) {
            console.error("Dashboard config load failed:", error);
            res.status(500).json({ error: error.message });
        }
    });

    app.put("/api/dashboard/config", requireAuth, async (req, res) => {
        try {
            const saved = await saveDashboardConfig(req.body || {});
            const sync = await syncReactionRoles().catch(error => ({ error: error.message }));

            res.json({
                config: sync.config || saved,
                sync
            });
        } catch (error) {
            console.error("Dashboard config save failed:", error);
            res.status(400).json({ error: error.message });
        }
    });

    app.post("/api/dashboard/reaction-roles/sync", requireAuth, async (req, res) => {
        const sync = await syncReactionRoles().catch(error => ({ error: error.message }));
        if (sync.error) {
            res.status(500).json(sync);
            return;
        }

        res.json(sync);
    });

    app.post("/api/dashboard/member-counts/sync", requireAuth, async (req, res) => {
        const sync = await syncMemberCountMessage().catch(error => ({ error: error.message }));
        if (sync.error) {
            res.status(500).json(sync);
            return;
        }

        res.json(sync);
    });

    app.post("/api/dashboard/recruitment/panel/sync", requireAuth, async (req, res) => {
        const sync = await ensureRecruitmentPanel().catch(error => ({ error: error.message }));
        if (sync.error) {
            res.status(500).json(sync);
            return;
        }

        const config = await loadDashboardConfig();
        res.json({ config, sync });
    });

    app.post("/api/dashboard/recruitment/ban-list/sync", requireAuth, async (req, res) => {
        const sync = await syncRecruitmentBanList().catch(error => ({ error: error.message }));
        if (sync.error) {
            res.status(500).json(sync);
            return;
        }

        const config = sync.config || await loadDashboardConfig();
        res.json({ config, sync });
    });

    app.get("/api/dashboard/recruitment/logs", requireAuth, async (req, res) => {
        res.json({ logs: await listRecruitmentLogs(100) });
    });

    app.get("/api/dashboard/tickets", requireAuth, async (req, res) => {
        const status = typeof req.query.status === "string" ? req.query.status : "";
        res.json({ tickets: await listTickets(status ? { status } : {}) });
    });

    app.get("/api/dashboard/tickets/:threadId/transcript", requireAuth, async (req, res) => {
        const threadId = String(req.params.threadId || "");
        let ticket = await getTicket(threadId);
        if (!ticket) {
            res.status(404).json({ error: "Ticket not found." });
            return;
        }

        let transcript = ticket.transcript || null;
        if (!transcript?.text) {
            transcript = await fetchDiscordTranscript(threadId).catch(() => null);
            if (transcript?.text) {
                ticket = await updateTicket(threadId, {
                    transcript,
                    transcriptSaved: true,
                    transcriptPreview: transcript.text.slice(0, 10000)
                }) || ticket;
            }
        }

        res.json({
            threadId: ticket.threadId,
            applicantTag: ticket.applicantTag || ticket.applicantId,
            status: ticket.status,
            outcome: ticket.team || ticket.outcome || "",
            closedAt: ticket.closedAt || "",
            transcript,
            transcriptPreview: ticket.transcriptPreview || transcript?.text?.slice(0, 10000) || "",
            applicantThreadImages: ticket.applicantThreadImages || []
        });
    });

    app.get("/api/dashboard/logs", requireAuth, async (req, res) => {
        const type = typeof req.query.type === "string" ? req.query.type : "";
        const limit = Number(req.query.limit || 150);
        const [botLogs, recruitmentLogs] = await Promise.all([
            listLogs(limit, type),
            listRecruitmentLogs(limit)
        ]);

        res.json({ botLogs, recruitmentLogs });
    });

    app.post("/api/dashboard/recruitment/tutorials/:id/upload", requireAuth, rawVideoUpload, async (req, res) => {
        try {
            const tutorialId = sanitizeFilename(req.params.id);
            const config = await loadDashboardConfig();
            const tutorial = config.recruitment.tutorials.find(item => item.id === tutorialId);
            if (!tutorial) throw new Error("Tutorial not found.");

            const videoUrl = await saveTutorialUpload(req, tutorialId, config);
            const nextTutorials = config.recruitment.tutorials.map(item =>
                item.id === tutorialId ? { ...item, videoUrl } : item
            );

            const saved = await saveDashboardConfig({
                ...config,
                recruitment: {
                    ...config.recruitment,
                    tutorials: nextTutorials
                }
            });

            res.json({
                config: saved,
                tutorial: saved.recruitment.tutorials.find(item => item.id === tutorialId)
            });
        } catch (error) {
            console.error("Tutorial upload failed:", error);
            res.status(400).json({ error: error.message });
        }
    });
}

module.exports = {
    registerDashboardRoutes
};
