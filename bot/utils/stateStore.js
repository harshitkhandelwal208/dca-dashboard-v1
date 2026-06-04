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
let poolKey = "";
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

function databaseClientType(connectionString) {
    const forced = String(process.env.DATABASE_CLIENT || process.env.DATABASE_TYPE || "").trim().toLowerCase();
    if (["mssql", "sqlserver", "sql-server", "azure-sql"].includes(forced)) return "sqlserver";
    if (["pg", "postgres", "postgresql"].includes(forced)) return "postgres";

    const text = String(connectionString || "").trim();
    if (/^(mssql|sqlserver):\/\//i.test(text)) return "sqlserver";
    if (/(^|;)\s*(server|data source)\s*=/i.test(text)) return "sqlserver";
    return "postgres";
}

function parseKeyValueConnectionString(connectionString) {
    const values = {};
    for (const part of String(connectionString || "").split(";")) {
        const index = part.indexOf("=");
        if (index === -1) continue;
        const key = part.slice(0, index).trim().toLowerCase().replace(/\s+/g, "");
        const value = part.slice(index + 1).trim();
        if (key) values[key] = value;
    }
    return values;
}

function parseSqlServerHost(value) {
    let text = String(value || "").trim().replace(/^tcp:/i, "");
    let port = 1433;

    const commaIndex = text.lastIndexOf(",");
    const colonIndex = text.lastIndexOf(":");
    const splitIndex = commaIndex > -1 ? commaIndex : colonIndex;
    if (splitIndex > -1) {
        const parsedPort = Number(text.slice(splitIndex + 1));
        if (Number.isInteger(parsedPort) && parsedPort > 0) {
            port = parsedPort;
            text = text.slice(0, splitIndex);
        }
    }

    return { server: text, port };
}

function sqlServerConfig(connectionString) {
    const text = String(connectionString || "").trim();
    if (/^(mssql|sqlserver):\/\//i.test(text)) {
        const parsed = new URL(text.replace(/^sqlserver:/i, "mssql:"));
        return {
            server: parsed.hostname,
            port: parsed.port ? Number(parsed.port) : 1433,
            database: decodeURIComponent(parsed.pathname.replace(/^\/+/, "")),
            user: decodeURIComponent(parsed.username),
            password: decodeURIComponent(parsed.password),
            options: {
                encrypt: parsed.searchParams.get("encrypt") !== "false",
                trustServerCertificate: parsed.searchParams.get("trustServerCertificate") === "true"
            },
            pool: { max: 5, min: 0, idleTimeoutMillis: 30000 },
            connectionTimeout: 30000,
            requestTimeout: 30000
        };
    }

    const values = parseKeyValueConnectionString(text);
    const host = parseSqlServerHost(values.server || values.datasource || values.addr || values.address || values.networkaddress);
    const timeoutSeconds = Number(values.connectiontimeout || values.connecttimeout || 30);

    return {
        server: host.server,
        port: host.port,
        database: values.initialcatalog || values.database,
        user: values.userid || values.uid || values.user,
        password: values.password || values.pwd,
        options: {
            encrypt: String(values.encrypt || "true").toLowerCase() !== "false",
            trustServerCertificate: String(values.trustservercertificate || "false").toLowerCase() === "true"
        },
        pool: { max: 5, min: 0, idleTimeoutMillis: 30000 },
        connectionTimeout: Number.isFinite(timeoutSeconds) ? timeoutSeconds * 1000 : 30000,
        requestTimeout: 30000
    };
}

function getDatabase() {
    const connectionString = getDatabaseUrl();
    if (!connectionString) return null;

    const type = databaseClientType(connectionString);
    const key = `${type}:${connectionString}`;
    if (!pool || poolKey !== key) {
        tableReady = false;
        poolKey = key;

        if (type === "sqlserver") {
            const sql = require("mssql");
            pool = new sql.ConnectionPool(sqlServerConfig(connectionString)).connect();
        } else {
            const { Pool } = require("pg");
            pool = new Pool({
                connectionString,
                ssl: shouldUseSsl(connectionString) ? { rejectUnauthorized: false } : undefined
            });
        }
    }

    return { type, pool };
}

function quotePostgresIdentifier(identifier) {
    return `"${String(identifier).replace(/"/g, "\"\"")}"`;
}

function quoteSqlServerIdentifier(identifier) {
    return `[${String(identifier).replace(/]/g, "]]")}]`;
}

function sqlServerTableName() {
    const schema = process.env.STATE_TABLE_SCHEMA || "dbo";
    return `${quoteSqlServerIdentifier(schema)}.${quoteSqlServerIdentifier(STATE_TABLE)}`;
}

async function ensureStateTable() {
    const db = getDatabase();
    if (!db || tableReady) return;

    if (db.type === "sqlserver") {
        const sqlPool = await db.pool;
        const schema = String(process.env.STATE_TABLE_SCHEMA || "dbo").replace(/'/g, "''");
        const table = String(STATE_TABLE).replace(/'/g, "''");
        await sqlPool.request().query(`
            IF OBJECT_ID(N'${schema}.${table}', N'U') IS NULL
            BEGIN
                CREATE TABLE ${sqlServerTableName()} (
                    scope nvarchar(200) NOT NULL PRIMARY KEY,
                    data nvarchar(max) NOT NULL,
                    updated_at datetimeoffset NOT NULL DEFAULT SYSUTCDATETIME(),
                    CONSTRAINT ${quoteSqlServerIdentifier(`${STATE_TABLE}_data_is_json`)} CHECK (ISJSON(data) = 1)
                )
            END
        `);
    } else {
        await db.pool.query(`
            CREATE TABLE IF NOT EXISTS ${quotePostgresIdentifier(STATE_TABLE)} (
                scope text PRIMARY KEY,
                data jsonb NOT NULL,
                updated_at timestamptz NOT NULL DEFAULT now()
            )
        `);
    }

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
    const db = getDatabase();
    if (!db) return readJson(scope, fallback);

    await ensureStateTable();
    if (db.type === "sqlserver") {
        const sql = require("mssql");
        const sqlPool = await db.pool;
        const result = await sqlPool.request()
            .input("scope", sql.NVarChar(200), scope)
            .query(`SELECT data FROM ${sqlServerTableName()} WHERE scope = @scope`);

        if (!result.recordset.length) return clone(fallback);

        try {
            return JSON.parse(result.recordset[0].data);
        } catch (error) {
            console.error(`Failed to parse ${scope} SQL state:`, error.message);
            return clone(fallback);
        }
    }

    const result = await db.pool.query(
        `SELECT data FROM ${quotePostgresIdentifier(STATE_TABLE)} WHERE scope = $1`,
        [scope]
    );

    if (!result.rowCount) return clone(fallback);
    return result.rows[0].data || clone(fallback);
}

async function writeState(scope, data) {
    const db = getDatabase();
    if (!db) return writeJson(scope, data);

    await ensureStateTable();
    const payload = JSON.stringify(data);

    if (db.type === "sqlserver") {
        const sql = require("mssql");
        const sqlPool = await db.pool;
        await sqlPool.request()
            .input("scope", sql.NVarChar(200), scope)
            .input("data", sql.NVarChar(sql.MAX), payload)
            .query(`
                MERGE ${sqlServerTableName()} WITH (HOLDLOCK) AS target
                USING (SELECT @scope AS scope, @data AS data) AS source
                ON target.scope = source.scope
                WHEN MATCHED THEN
                    UPDATE SET data = source.data, updated_at = SYSUTCDATETIME()
                WHEN NOT MATCHED THEN
                    INSERT (scope, data, updated_at)
                    VALUES (source.scope, source.data, SYSUTCDATETIME());
            `);
    } else {
        await db.pool.query(
            `
                INSERT INTO ${quotePostgresIdentifier(STATE_TABLE)} (scope, data, updated_at)
                VALUES ($1, $2::jsonb, now())
                ON CONFLICT (scope)
                DO UPDATE SET data = EXCLUDED.data, updated_at = now()
            `,
            [scope, payload]
        );
    }

    return data;
}

module.exports = {
    readState,
    writeState,
    filePathFor
};
