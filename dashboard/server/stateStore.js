const fs = require("fs");
const path = require("path");

const STATE_COLLECTION = process.env.FIREBASE_STATE_COLLECTION || "dca_bot_state";
const STATE_ROOT = process.env.FIREBASE_STATE_ROOT || STATE_COLLECTION;
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
        : path.join(__dirname, "..", "data", "botLogs.json"),
    warnings: () => process.env.WARNINGS_PATH
        ? path.resolve(process.env.WARNINGS_PATH)
        : path.join(__dirname, "..", "data", "warnings.json")
};

let firebaseStore;

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function firebaseConfigured() {
    return Boolean(
        process.env.FIREBASE_SERVICE_ACCOUNT ||
        process.env.FIREBASE_SERVICE_ACCOUNT_PATH ||
        process.env.GOOGLE_APPLICATION_CREDENTIALS ||
        process.env.FIREBASE_PROJECT_ID
    );
}

function parseServiceAccount() {
    if (process.env.FIREBASE_SERVICE_ACCOUNT_PATH) {
        const raw = fs.readFileSync(path.resolve(process.env.FIREBASE_SERVICE_ACCOUNT_PATH), "utf8");
        return JSON.parse(raw);
    }

    if (!process.env.FIREBASE_SERVICE_ACCOUNT) return null;

    const raw = process.env.FIREBASE_SERVICE_ACCOUNT.trim();
    const json = raw.startsWith("{")
        ? raw
        : Buffer.from(raw, "base64").toString("utf8");

    return JSON.parse(json);
}

function firebaseDatabaseType() {
    const forced = String(process.env.FIREBASE_DATABASE_TYPE || "").trim().toLowerCase();
    if (["realtime", "rtdb", "database"].includes(forced)) return "realtime";
    if (["firestore", "cloud-firestore"].includes(forced)) return "firestore";
    return process.env.FIREBASE_DATABASE_URL ? "realtime" : "firestore";
}

function getFirebaseStore() {
    if (!firebaseConfigured()) return null;
    if (firebaseStore) return firebaseStore;

    const admin = require("firebase-admin");
    const type = firebaseDatabaseType();

    if (!admin.apps.length) {
        const serviceAccount = parseServiceAccount();
        const options = {};

        if (serviceAccount) {
            options.credential = admin.credential.cert(serviceAccount);
            options.projectId = process.env.FIREBASE_PROJECT_ID || serviceAccount.project_id;
        } else {
            options.credential = admin.credential.applicationDefault();
            if (process.env.FIREBASE_PROJECT_ID) options.projectId = process.env.FIREBASE_PROJECT_ID;
        }

        if (process.env.FIREBASE_DATABASE_URL) {
            options.databaseURL = process.env.FIREBASE_DATABASE_URL;
        }

        admin.initializeApp(options);
    }

    firebaseStore = type === "realtime"
        ? { type, db: admin.database() }
        : { type, db: admin.firestore() };

    return firebaseStore;
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
    const store = getFirebaseStore();
    if (!store) return readJson(scope, fallback);

    if (store.type === "realtime") {
        const snapshot = await store.db.ref(`${STATE_ROOT}/${scope}`).get();
        if (!snapshot.exists()) return clone(fallback);

        const state = snapshot.val();
        return state && Object.prototype.hasOwnProperty.call(state, "data")
            ? state.data
            : clone(fallback);
    }

    const snapshot = await store.db.collection(STATE_COLLECTION).doc(scope).get();
    if (!snapshot.exists) return clone(fallback);

    const state = snapshot.data();
    return state && Object.prototype.hasOwnProperty.call(state, "data")
        ? state.data
        : clone(fallback);
}

async function writeState(scope, data) {
    const store = getFirebaseStore();
    if (!store) return writeJson(scope, data);

    const payload = {
        data,
        updatedAt: new Date().toISOString()
    };

    if (store.type === "realtime") {
        await store.db.ref(`${STATE_ROOT}/${scope}`).set(payload);
        return data;
    }

    await store.db.collection(STATE_COLLECTION).doc(scope).set(payload, { merge: true });

    return data;
}

module.exports = {
    readState,
    writeState,
    filePathFor
};
