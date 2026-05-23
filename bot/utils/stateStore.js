const fs = require("fs");
const path = require("path");

const STATE_TABLE = process.env.STATE_TABLE_NAME || "dca_bot_state";
const JSON_PATHS = {
    dashboardConfig: () => process.env.DASHBOARD_CONFIG_PATH
        ? path.resolve(process.env.DASHBOARD_CONFIG_PATH)
        : path.join(__dirname, "..", "data", "dashboardConfig.json"),
    recruitmentTickets: () => process.env.RECRUITMENT_TICKETS_PATH
        ? path.resolve(process.env.RECRUITMENT_TICKETS_PATH)
        : path.join(__dirname, "..", "data", "recruitmentTickets.json"),
    recruitmentLogs: () => process.env.RECRUITMENT_LOGS_PATH
        ? path.resolve(process.env.RECRUITMENT_LOGS_PATH)
        : path.join(__dirname, "..", "data", "recruitmentLogs.json"),
    recruitmentBans: () => process.env.RECRUITMENT_BANS_PATH
        ? path.resolve(process.env.RECRUITMENT_BANS_PATH)
        : path.join(__dirname, "..", "data", "recruitmentBans.json"),
    botLogs: () => process.env.BOT_LOGS_PATH
        ? path.resolve(process.env.BOT_LOGS_PATH)
        : path.join(__dirname, "..", "data", "botLogs.json")
};

let pool;
let tableReady = false;

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function getDatabaseUrl() {
    return process.env.DATABASE_URL || "";
}

function shouldUseSsl(connectionString) {
    if (process.env.DATABASE_SSL) {
        return process.env.DATABASE_SSL.toLowerCase() !== "false";
    }

    return /sslmode=require|neon\.tech|supabase\.co|render\.com/i.test(connectionString);
}

function getPool() {
    const connectionString = getDatabaseUrl();
    if (!connectionString) return null;

    if (!pool) {
        const { Pool } = require("pg");
        pool = new Pool({
            connectionString,
            ssl: shouldUseSsl(connectionString) ? { rejectUnauthorized: false } : undefined
        });
    }

    return pool;
}

function quoteIdentifier(identifier) {
    return `"${String(identifier).replace(/"/g, "\"\"")}"`;
}

async function ensureStateTable() {
    const db = getPool();
    if (!db || tableReady) return;

    await db.query(`
        CREATE TABLE IF NOT EXISTS ${quoteIdentifier(STATE_TABLE)} (
            scope text PRIMARY KEY,
            data jsonb NOT NULL,
            updated_at timestamptz NOT NULL DEFAULT now()
        )
    `);
    tableReady = true;
}

function filePathFor(scope) {
    const factory = JSON_PATHS[scope];
    if (factory) return factory();
    return path.join(__dirname, "..", "data", `${scope}.json`);
}

async function readJson(scope, fallback) {
    const filePath = filePathFor(scope);

    try {
        if (!fs.existsSync(filePath)) return clone(fallback);

        const raw = await fs.promises.readFile(filePath, "utf8");
        if (!raw.trim()) return clone(fallback);

        return JSON.parse(raw);
    } catch (error) {
        console.error(`Failed to read ${scope} JSON store:`, error.message);
        return clone(fallback);
    }
}

async function writeJson(scope, data) {
    const filePath = filePathFor(scope);
    const dir = path.dirname(filePath);
    await fs.promises.mkdir(dir, { recursive: true });

    const tempPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
    await fs.promises.writeFile(tempPath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
    await fs.promises.rename(tempPath, filePath);
    return data;
}

async function readState(scope, fallback) {
    const db = getPool();
    if (!db) return readJson(scope, fallback);

    await ensureStateTable();
    const result = await db.query(
        `SELECT data FROM ${quoteIdentifier(STATE_TABLE)} WHERE scope = $1`,
        [scope]
    );

    if (!result.rowCount) return clone(fallback);
    return result.rows[0].data || clone(fallback);
}

async function writeState(scope, data) {
    const db = getPool();
    if (!db) return writeJson(scope, data);

    await ensureStateTable();
    await db.query(
        `
            INSERT INTO ${quoteIdentifier(STATE_TABLE)} (scope, data, updated_at)
            VALUES ($1, $2::jsonb, now())
            ON CONFLICT (scope)
            DO UPDATE SET data = EXCLUDED.data, updated_at = now()
        `,
        [scope, JSON.stringify(data)]
    );

    return data;
}

module.exports = {
    readState,
    writeState,
    filePathFor
};
