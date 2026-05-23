const crypto = require("crypto");
const { getGuildMembers } = require("./discordApi");
const { loadDashboardConfig } = require("./dashboardConfig");

const DISCORD_API = "https://discord.com/api/v10";
const SESSION_COOKIE = "dca_dashboard_session";
const STATE_COOKIE = "dca_dashboard_state";
const SESSION_TTL_MS = Math.max(1, Number(process.env.DASHBOARD_SESSION_HOURS || 8)) * 60 * 60 * 1000;
const STATE_TTL_MS = 10 * 60 * 1000;
const ROLE_RECHECK_MS = Math.max(1, Number(process.env.DASHBOARD_ROLE_RECHECK_MINUTES || 5)) * 60 * 1000;
const ROLE_RECHECK_GRACE_MS = Math.max(1, Number(process.env.DASHBOARD_ROLE_RECHECK_GRACE_MINUTES || 30)) * 60 * 1000;

const sessionSecret = process.env.DASHBOARD_SESSION_SECRET || crypto.randomBytes(32).toString("hex");
const sessions = new Map();

if (!process.env.DASHBOARD_SESSION_SECRET) {
    console.warn("DASHBOARD_SESSION_SECRET is missing. Dashboard sessions will reset when the process restarts.");
}

function getAllowedRoleIdFromEnv() {
    return process.env.DASHBOARD_ALLOWED_ROLE_ID ||
        process.env.DISCORD_DASHBOARD_ROLE_ID ||
        process.env.DASHBOARD_ROLE_ID ||
        "";
}

async function getAllowedRoleId() {
    const config = await loadDashboardConfig().catch(() => null);
    return config?.bot?.dashboardAllowedRoleId || getAllowedRoleIdFromEnv() || "";
}

function getBaseUrl(req) {
    if (process.env.DASHBOARD_BASE_URL) return process.env.DASHBOARD_BASE_URL.replace(/\/$/, "");

    const protocol = req.headers["x-forwarded-proto"] || req.protocol || "http";
    return `${String(protocol).split(",")[0]}://${req.get("host")}`;
}

async function getOAuthConfig(req) {
    const dashboardConfig = await loadDashboardConfig().catch(() => null);
    const guildIds = [...new Set([
        dashboardConfig?.bot?.communityGuildId,
        dashboardConfig?.bot?.recruitmentGuildId,
        dashboardConfig?.bot?.guildId,
        process.env.COMMUNITY_GUILD_ID,
        process.env.RECRUITMENT_GUILD_ID,
        process.env.DISCORD_GUILD_ID
    ].filter(Boolean))];
    const config = {
        clientId: process.env.DISCORD_CLIENT_ID || "",
        clientSecret: process.env.DISCORD_CLIENT_SECRET || "",
        guildId: guildIds[0] || "",
        guildIds,
        allowedRoleId: dashboardConfig?.bot?.dashboardAllowedRoleId || getAllowedRoleIdFromEnv() || "",
        redirectUri: process.env.DISCORD_REDIRECT_URI || `${getBaseUrl(req)}/auth/discord/callback`
    };

    const missing = [];
    if (!config.clientId) missing.push("DISCORD_CLIENT_ID");
    if (!config.clientSecret) missing.push("DISCORD_CLIENT_SECRET");
    if (!config.guildIds.length) missing.push("DISCORD_GUILD_ID");
    if (!config.allowedRoleId) missing.push("DASHBOARD_ALLOWED_ROLE_ID");
    if (!process.env.DISCORD_TOKEN) missing.push("DISCORD_TOKEN");

    return { ...config, missing, configured: missing.length === 0 };
}

function parseCookies(req) {
    const header = req.headers.cookie || "";
    const cookies = {};

    for (const part of header.split(";")) {
        const index = part.indexOf("=");
        if (index === -1) continue;

        const name = part.slice(0, index).trim();
        const value = part.slice(index + 1).trim();
        if (!name) continue;

        try {
            cookies[name] = decodeURIComponent(value);
        } catch {
            cookies[name] = value;
        }
    }

    return cookies;
}

function appendSetCookie(res, cookie) {
    const existing = res.getHeader("Set-Cookie");
    if (!existing) {
        res.setHeader("Set-Cookie", cookie);
    } else if (Array.isArray(existing)) {
        res.setHeader("Set-Cookie", [...existing, cookie]);
    } else {
        res.setHeader("Set-Cookie", [existing, cookie]);
    }
}

function isSecureRequest(req) {
    return Boolean(
        req.secure ||
        String(req.headers["x-forwarded-proto"] || "").split(",")[0] === "https" ||
        String(process.env.DASHBOARD_BASE_URL || "").startsWith("https://")
    );
}

function setCookie(req, res, name, value, maxAgeMs) {
    const parts = [
        `${name}=${encodeURIComponent(value)}`,
        "Path=/",
        "HttpOnly",
        "SameSite=Lax"
    ];

    if (Number.isFinite(maxAgeMs)) parts.push(`Max-Age=${Math.floor(maxAgeMs / 1000)}`);
    if (isSecureRequest(req)) parts.push("Secure");

    appendSetCookie(res, parts.join("; "));
}

function clearCookie(req, res, name) {
    setCookie(req, res, name, "", 0);
}

function publicErrorDetail(message) {
    const text = String(message || "");

    if (/state/i.test(text)) return "oauth_state";
    if (/missing dashboard oauth settings/i.test(text)) return "missing_setup";
    if (/invalid_grant|invalid client|client_secret|unauthorized_client/i.test(text)) return "oauth_exchange";
    if (/access token/i.test(text)) return "missing_access_token";
    if (/unknown guild|guild|missing access|404/i.test(text)) return "guild_member_lookup";
    if (/401|unauthorized/i.test(text)) return "discord_unauthorized";
    if (/429|rate/i.test(text)) return "discord_rate_limited";

    return "discord_callback";
}

function safeOAuthDebug(req, oauthConfig, expectedState) {
    return {
        host: req.get("host"),
        forwardedProto: String(req.headers["x-forwarded-proto"] || ""),
        redirectUri: oauthConfig.redirectUri,
        guildId: oauthConfig.guildId || "",
        guildIds: oauthConfig.guildIds || [],
        allowedRoleId: oauthConfig.allowedRoleId || "",
        hasClientId: Boolean(oauthConfig.clientId),
        hasClientSecret: Boolean(oauthConfig.clientSecret),
        hasBotToken: Boolean(process.env.DISCORD_TOKEN),
        receivedCode: Boolean(req.query.code),
        receivedState: Boolean(req.query.state),
        expectedState: Boolean(expectedState)
    };
}

function hmac(value) {
    return crypto.createHmac("sha256", sessionSecret).update(value).digest("base64url");
}

function sign(value) {
    return `${value}.${hmac(value)}`;
}

function unsign(signedValue) {
    if (typeof signedValue !== "string") return "";

    const index = signedValue.lastIndexOf(".");
    if (index === -1) return "";

    const value = signedValue.slice(0, index);
    const signature = signedValue.slice(index + 1);
    const expected = hmac(value);

    const signatureBuffer = Buffer.from(signature);
    const expectedBuffer = Buffer.from(expected);
    if (signatureBuffer.length !== expectedBuffer.length) return "";

    return crypto.timingSafeEqual(signatureBuffer, expectedBuffer) ? value : "";
}

function setSignedCookie(req, res, name, value, maxAgeMs) {
    setCookie(req, res, name, sign(value), maxAgeMs);
}

function getSignedCookie(req, name) {
    return unsign(parseCookies(req)[name]);
}

async function discordRequest(url, options) {
    const response = await fetch(url, options);
    const text = await response.text();
    let data = {};

    if (text) {
        try {
            data = JSON.parse(text);
        } catch {
            data = { message: text };
        }
    }

    if (!response.ok) {
        const error = new Error(data.error_description || data.error || data.message || `Discord API returned ${response.status}`);
        error.status = response.status;
        throw error;
    }

    return data;
}

async function exchangeCodeForToken(code, oauthConfig) {
    const body = new URLSearchParams({
        client_id: oauthConfig.clientId,
        client_secret: oauthConfig.clientSecret,
        grant_type: "authorization_code",
        code,
        redirect_uri: oauthConfig.redirectUri
    });

    return discordRequest(`${DISCORD_API}/oauth2/token`, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body
    });
}

async function getDiscordUser(accessToken) {
    return discordRequest(`${DISCORD_API}/users/@me`, {
        headers: { Authorization: `Bearer ${accessToken}` }
    });
}

async function getDiscordGuildMember(accessToken, guildId) {
    return discordRequest(`${DISCORD_API}/users/@me/guilds/${guildId}/member`, {
        headers: { Authorization: `Bearer ${accessToken}` }
    });
}

async function getDiscordGuildMembers(accessToken, guildIds) {
    const members = [];
    const errors = [];

    for (const guildId of guildIds || []) {
        try {
            members.push({ guildId, member: await getDiscordGuildMember(accessToken, guildId) });
        } catch (error) {
            errors.push({ guildId, status: error.status || 0, error: error.message });
        }
    }

    return { members, errors };
}

function createSession(req, res, user, memberRoles) {
    const sessionId = crypto.randomBytes(32).toString("base64url");
    const now = Date.now();

    sessions.set(sessionId, {
        user: {
            id: user.id,
            username: user.username,
            globalName: user.global_name || "",
            avatar: user.avatar || ""
        },
        memberRoles: Array.isArray(memberRoles) ? memberRoles : [],
        createdAt: now,
        expiresAt: now + SESSION_TTL_MS,
        lastRoleCheckAt: now
    });

    setSignedCookie(req, res, SESSION_COOKIE, sessionId, SESSION_TTL_MS);
    return sessionId;
}

function getSession(req) {
    const sessionId = getSignedCookie(req, SESSION_COOKIE);
    if (!sessionId) return null;

    const session = sessions.get(sessionId);
    if (!session) return null;

    if (Date.now() > session.expiresAt) {
        sessions.delete(sessionId);
        return null;
    }

    return { id: sessionId, ...session };
}

function destroySession(req, res) {
    const sessionId = getSignedCookie(req, SESSION_COOKIE);
    if (sessionId) sessions.delete(sessionId);
    clearCookie(req, res, SESSION_COOKIE);
}

async function sessionStillHasRole(session) {
    const allowedRoleId = await getAllowedRoleId();
    if (!allowedRoleId) return false;

    const cachedRoles = Array.isArray(session.memberRoles) ? session.memberRoles : [];
    if (Date.now() - session.lastRoleCheckAt < ROLE_RECHECK_MS && cachedRoles.includes(allowedRoleId)) return true;

    try {
        const { members, errors } = await getGuildMembers(session.user.id);
        const transientErrors = errors.filter(error => ![403, 404].includes(Number(error.status)));
        if (!members.length && transientErrors.length) {
            throw new Error(transientErrors.map(error => error.error).join("; "));
        }

        const roles = [...new Set(members.flatMap(({ member }) => Array.isArray(member.roles) ? member.roles : []))];
        const ok = roles.includes(allowedRoleId);
        const stored = sessions.get(session.id);

        if (stored) {
            stored.lastRoleCheckAt = Date.now();
            stored.memberRoles = roles;
        }

        return ok;
    } catch (error) {
        const lastVerifiedAt = Number(session.lastRoleCheckAt || 0);
        const recentlyVerified = lastVerifiedAt > 0 && Date.now() - lastVerifiedAt < ROLE_RECHECK_GRACE_MS;

        if (recentlyVerified && cachedRoles.includes(allowedRoleId)) {
            console.warn("Dashboard role recheck failed; using cached verified role:", error.message);
            return true;
        }

        console.error("Dashboard role recheck failed:", error.message);
        return null;
    }
}

function requireDashboardAuth() {
    return async (req, res, next) => {
        const session = getSession(req);
        if (!session) {
            res.status(401).json({ error: "Discord sign in required." });
            return;
        }

        const roleOk = await sessionStillHasRole(session);
        if (roleOk === false) {
            destroySession(req, res);
            res.status(403).json({ error: "Required Discord role is missing." });
            return;
        }

        if (roleOk === null) {
            res.status(503).json({ error: "Could not verify Discord role right now." });
            return;
        }

        req.dashboardUser = session.user;
        next();
    };
}

function registerDashboardAuthRoutes(app) {
    app.get("/auth/discord", async (req, res) => {
        const oauthConfig = await getOAuthConfig(req);
        if (!oauthConfig.configured) {
            res.redirect("/dashboard?auth=setup");
            return;
        }

        const state = crypto.randomBytes(24).toString("base64url");
        setSignedCookie(req, res, STATE_COOKIE, state, STATE_TTL_MS);

        const params = new URLSearchParams({
            response_type: "code",
            client_id: oauthConfig.clientId,
            scope: "identify guilds.members.read",
            redirect_uri: oauthConfig.redirectUri,
            state,
            prompt: "consent"
        });

        res.redirect(`https://discord.com/oauth2/authorize?${params.toString()}`);
    });

    app.get("/auth/discord/callback", async (req, res) => {
        const oauthConfig = await getOAuthConfig(req);
        const expectedState = getSignedCookie(req, STATE_COOKIE);
        clearCookie(req, res, STATE_COOKIE);

        try {
            if (!oauthConfig.configured) throw new Error(`Missing dashboard OAuth settings: ${oauthConfig.missing.join(", ")}`);
            if (!req.query.code || !req.query.state || req.query.state !== expectedState) {
                throw new Error("Invalid Discord OAuth state.");
            }

            const token = await exchangeCodeForToken(String(req.query.code), oauthConfig);
            const accessToken = token.access_token;
            if (!accessToken) throw new Error("Discord did not return an access token.");

            const [user, memberResults] = await Promise.all([
                getDiscordUser(accessToken),
                getDiscordGuildMembers(accessToken, oauthConfig.guildIds)
            ]);

            const transientErrors = memberResults.errors.filter(error => ![403, 404].includes(Number(error.status)));
            if (!memberResults.members.length && transientErrors.length) {
                throw new Error(transientErrors.map(error => error.error).join("; "));
            }

            const roles = [...new Set(memberResults.members.flatMap(({ member }) => Array.isArray(member.roles) ? member.roles : []))];
            if (!roles.includes(oauthConfig.allowedRoleId)) {
                res.redirect("/dashboard?auth=denied");
                return;
            }

            createSession(req, res, user, roles);
            res.redirect("/dashboard");
        } catch (error) {
            const detail = publicErrorDetail(error.message);
            console.error("Discord dashboard sign in failed:", error.message, safeOAuthDebug(req, oauthConfig, expectedState));
            res.redirect(`/dashboard?auth=error&detail=${encodeURIComponent(detail)}`);
        }
    });

    app.post("/api/dashboard/logout", (req, res) => {
        destroySession(req, res);
        res.json({ ok: true });
    });

    app.get("/auth/logout", (req, res) => {
        destroySession(req, res);
        res.redirect("/dashboard");
    });
}

module.exports = {
    getOAuthConfig,
    getSession,
    registerDashboardAuthRoutes,
    requireDashboardAuth
};
