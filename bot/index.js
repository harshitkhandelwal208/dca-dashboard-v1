const { Client, GatewayIntentBits, Collection, Partials } = require("discord.js");
const fs = require("fs");
const path = require("path");
const envPath = path.resolve(__dirname, "../.env");
require("dotenv").config({
    path: fs.existsSync(envPath) ? envPath : path.resolve(__dirname, ".env")
});

const bundledFontConfig = path.join(__dirname, "fonts", "fonts.conf");
if (!process.env.FONTCONFIG_FILE && fs.existsSync(bundledFontConfig)) {
    process.env.FONTCONFIG_FILE = bundledFontConfig;
}
if (!process.env.FONTCONFIG_PATH && fs.existsSync(path.dirname(bundledFontConfig))) {
    process.env.FONTCONFIG_PATH = path.dirname(bundledFontConfig);
}

const {
    ensureReactionRoleMessages,
    handleReactionRole: handleDashboardReactionRole
} = require("./utils/reactionRoleManager");
const {
    ensureRecruitmentPanel,
    handleRecruitmentInteraction
} = require("./utils/recruitmentManager");
const { syncRecruitmentBanList } = require("./utils/recruitmentBanPanel");
const { syncMemberCountMessage } = require("./utils/memberCountManager");
const { startTeamRoleScheduler } = require("./utils/teamRoleScheduler");
const { handleWelcomeTeamButton } = require("./utils/welcomeRoleManager");
const { startYouTubeNotifier } = require("./utils/youtubeManager");
const { handleSpreadsheetMessage, startSpreadsheetReportScheduler } = require("./utils/spreadsheetManager");
const { registerDashboardRoutes } = require("../dashboard/server/dashboardRoutes");

// Error handlers
process.on("unhandledRejection", reason => console.error("Unhandled Rejection:", reason));
process.on("uncaughtException", err => console.error("Uncaught Exception:", err));

// Client Setup
const client = new Client({
    intents: [
        GatewayIntentBits.Guilds,
        GatewayIntentBits.GuildMessages,
        GatewayIntentBits.GuildMembers,
        GatewayIntentBits.GuildMessageReactions,
        GatewayIntentBits.GuildBans,
        GatewayIntentBits.MessageContent,
        GatewayIntentBits.DirectMessages
    ],
    partials: [
    Partials.Message,
    Partials.Channel,
    Partials.Reaction
  ]
});
// testing
client.on("warn", (info) => {
    console.warn("⚠️ Client warning:", info);
});

client.on("debug", (info) => {
    if (info.includes("error") || info.includes("connection")) {
        console.log("🔍 Debug:", info);
    }
});

// custom ready flag for health check
client.isBotReady = false;

client.commands = new Collection();
client.slashCommands = new Collection();

const activeButtonActions = new Map();
const BUTTON_ACTION_TTL_MS = 15 * 60 * 1000;

function buttonActionKey(interaction) {
    const customId = interaction.customId || "";

    if (customId === "recruitment:apply") {
        return `button:${interaction.guildId}:${interaction.user.id}:${customId}`;
    }

    if (customId === "recruitment:claim") {
        return `button:${interaction.guildId}:${interaction.channelId}:${customId}`;
    }

    if (customId === "recruitment:close") {
        return `button:${interaction.guildId}:${interaction.channelId}:${interaction.user.id}:${customId}`;
    }

    if (customId.startsWith("recruitment:close-team:")) {
        return `button:${interaction.guildId}:${interaction.channelId}:recruitment:close-team`;
    }

    if (customId.startsWith("recruitment:event:")) {
        return `button:${interaction.guildId}:${interaction.user.id}:${customId}`;
    }

    if (customId.startsWith("recruitment:tutorial:")) {
        return `button:${interaction.guildId}:${interaction.channelId}:${interaction.user.id}:${customId}`;
    }

    if (customId.startsWith("welcome-team:")) {
        return `button:${interaction.guildId}:${customId}`;
    }

    return `button:${interaction.id}`;
}

async function replyActionInProgress(interaction) {
    const payload = { content: "That button action is already being processed.", ephemeral: true };
    if (interaction.deferred || interaction.replied) {
        await interaction.followUp(payload).catch(() => null);
    } else {
        await interaction.reply(payload).catch(() => null);
    }
}

async function withButtonActionLock(interaction, action) {
    const key = buttonActionKey(interaction);
    if (activeButtonActions.has(key)) {
        await replyActionInProgress(interaction);
        return true;
    }

    const timeout = setTimeout(() => activeButtonActions.delete(key), BUTTON_ACTION_TTL_MS);
    activeButtonActions.set(key, timeout);

    try {
        return await action();
    } finally {
        clearTimeout(timeout);
        activeButtonActions.delete(key);
    }
}

// Load text (-) commands
const textCommandsPath = path.join(__dirname, "commands", "text");
if (fs.existsSync(textCommandsPath)) {
    const textFiles = fs.readdirSync(textCommandsPath).filter(f => f.endsWith(".js"));
    for (const file of textFiles) {
        try {
            const command = require(`./commands/text/${file}`);

            // 🔧 FIXED PART (this is the only change)
            if (command.name && command.execute) {
                command.description = command.description || "No description";
                client.commands.set(command.name, command);
                console.log(`✅ Loaded text command: ${command.name}`);
            } else {
                console.warn(`⚠️ Invalid command in ${file}`);
            }

        } catch (err) {
            console.error(`❌ Error loading text command ${file}:`, err);
        }
    }
}

// Load slash (/) commands
const slashPath = path.join(__dirname, "commands", "slash");

function getAllSlashCommands(dir) {
    let results = [];
    for (const file of fs.readdirSync(dir)) {
        const full = path.join(dir, file);
        if (fs.statSync(full).isDirectory()) {
            results = results.concat(getAllSlashCommands(full));
        } else if (file.endsWith(".js")) {
            results.push(full);
        }
    }
    return results;
}

if (fs.existsSync(slashPath)) {
    const slashFiles = getAllSlashCommands(slashPath);
    for (const file of slashFiles) {
        try {
            const command = require(file);
            if (command.data && command.data.name) {
                client.slashCommands.set(command.data.name, command);
                console.log(`✅ Loaded slash command: ${command.data.name}`);
            } else {
                console.warn(`⚠️ Missing slash command name in ${file}`);
            }
        } catch (err) {
            console.error(`❌ Error loading slash command ${file}:`, err);
        }
    }
}

// Load event files
const eventsPath = path.join(__dirname, "events");
if (fs.existsSync(eventsPath)) {
    const eventFiles = fs.readdirSync(eventsPath).filter(f => f.endsWith(".js"));
    for (const file of eventFiles) {
        try {
            const event = require(`./events/${file}`);
            if (event.name) {
                if (event.once) {
                    client.once(event.name, (...args) => event.execute(...args, client));
                } else {
                    client.on(event.name, (...args) => event.execute(...args, client));
                }
                console.log(`✅ Loaded event: ${event.name}`);
            } else {
                console.warn(`⚠️ Missing event name in ${file}`);
            }
        } catch (err) {
            console.error(`❌ Error loading event ${file}:`, err);
        }
    }
}

// "-" text commands
client.on("messageCreate", async message => {
    try {
        await handleSpreadsheetMessage(message);
    } catch (err) {
        console.error("Spreadsheet message handler failed:", err);
    }

    if (!message.content.startsWith("-") || message.author.bot) return;

    const args = message.content.slice(1).trim().split(/ +/);
    const commandName = args.shift().toLowerCase();
    const command = client.commands.get(commandName);
    if (!command) return;

    try {
        await command.execute(message, args);
    } catch (err) {
        console.error(`❌ Error executing command ${commandName}:`, err);
        message.reply("❌ Error executing this command.");
    }
});

// Slash commands & buttons
client.on("interactionCreate", async interaction => {
    if (interaction.isCommand()) {
        const cmd = client.slashCommands.get(interaction.commandName);
        if (!cmd) return;

        try {
            await cmd.execute(interaction, client);
        } catch (err) {
            console.error(`❌ Slash error ${interaction.commandName}:`, err);
            const payload = { content: "Error executing this command.", ephemeral: true };
            if (interaction.deferred || interaction.replied) {
                interaction.followUp(payload).catch(() => null);
            } else {
                interaction.reply(payload).catch(() => null);
            }
        }
    }

    if (interaction.isButton()) {
        const handled = await withButtonActionLock(interaction, async () => {
            const recruitmentHandled = await handleRecruitmentInteraction(interaction);
            if (recruitmentHandled) return true;

            const welcomeHandled = await handleWelcomeTeamButton(interaction);
            if (welcomeHandled) return true;

            return false;
        });
        if (handled) return;
    }

    if (interaction.isModalSubmit()) {
        const handled = await handleRecruitmentInteraction(interaction);
        if (handled) return;
    }
});

// READY EVENT
client.once("ready", async () => {
    // mark bot as ready for health check
    client.isBotReady = true;

    console.log(`\n========================`);
    console.log(`✅ Logged in as ${client.user.tag}`);
    console.log(`========================\n`);

    console.log("ℹ️ Syncing dashboard-managed reaction role messages...");

    try {
        const sync = await Promise.race([
            ensureReactionRoleMessages(client),
            new Promise((_, reject) =>
                setTimeout(() => reject(new Error("Reaction role setup timeout after 20s")), 20000)
            )
        ]);

        const syncedCount = sync.results.filter(result => !result.skipped).length;
        console.log(`✅ Reaction role sync done (${syncedCount} messages).`);
    } catch (err) {
        console.error("❌ Reaction role script error:", err.message);
        client.reactionRoleMessageMap = new Map();
    }

    console.log("Syncing recruitment Apply panel...");

    try {
        const sync = await Promise.race([
            ensureRecruitmentPanel(client),
            new Promise((_, reject) =>
                setTimeout(() => reject(new Error("Recruitment panel sync timeout after 2s")), 2000)
            )
        ]);

        if (sync.skipped) {
            console.log(`Recruitment panel sync skipped: ${sync.reason}`);
        } else {
            console.log(`Recruitment panel ready in ${sync.channelId} (${sync.messageId}).`);
        }
    } catch (err) {
        console.error("Recruitment panel sync error:", err.message);
    }

    console.log("Syncing member count message...");

    try {
        const sync = await Promise.race([
            syncMemberCountMessage(client, { silent: true }),
            new Promise((_, reject) =>
                setTimeout(() => reject(new Error("Member count sync timeout after 20s")), 20000)
            )
        ]);

        if (sync.skipped) {
            console.log(`Member count sync skipped: ${sync.reason}`);
        } else {
            console.log(`Member count message ready in ${sync.channelId} (${sync.messageId}).`);
        }
    } catch (err) {
        console.error("Member count sync error:", err.message);
    }

    startYouTubeNotifier(client);
    startTeamRoleScheduler(client);
    startSpreadsheetReportScheduler(client);

    syncRecruitmentBanList(client).catch(error => {
        console.error("Recruitment ban list sync error:", error.message);
    });
});

// Handle unexpected disconnections
client.on("disconnect", () => {
    console.log("⚠️ Bot disconnected from Discord");
    client.isBotReady = false;
});

// Handle client errors
client.on("error", err => {
    console.error("❌ Discord client error:", err);
});

// Universal reaction-role handler
client.on("messageReactionAdd", (r, u) => handleDashboardReactionRole(client, r, u, true));
client.on("messageReactionRemove", (r, u) => handleDashboardReactionRole(client, r, u, false));

// Express keep-alive server
const express = require("express");
const app = express();
const PORT = process.env.PORT || 3000;

app.set("trust proxy", 1);
app.use(express.json({ limit: "512kb" }));

// The bot and dashboard share this single HTTP process and port.
registerDashboardRoutes(app);

app.get("/", (req, res) => {
    res.status(client.isBotReady ? 200 : 503).json({
        status: client.isBotReady ? "alive" : "disconnected",
        uptime: Math.floor(process.uptime()),
        discordStatus: client.isBotReady ? "connected" : "disconnected",
        timestamp: new Date().toISOString()
    });
});

// Health check endpoint for Render
app.get("/health", (req, res) => {
    if (client.isReady()) {
        res.status(200).json({
            status: "healthy",
            uptime: Math.floor(process.uptime()),
            discordStatus: "connected",
            timestamp: new Date().toISOString()
        });
    } else {
        res.status(503).json({
            status: "unhealthy",
            uptime: Math.floor(process.uptime()),
            discordStatus: "disconnected",
            timestamp: new Date().toISOString()
        });
    }
});

// Keep-alive logs every 5 min
setInterval(() => {
    console.log(`Keep-alive: Discord ${client.isBotReady ? "connected" : "DISCONNECTED"}`);
    ensureRecruitmentPanel(client).catch(err => console.error("Recruitment panel keep-alive error:", err.message));
}, 60 * 1000);

app.listen(PORT, () => console.log(`🌐 Web server running on ${PORT}`));

// Single login call
console.log("📡 Attempting Discord connection...");
console.log(`Token configured: ${process.env.DISCORD_TOKEN ? "yes" : "no"}`);

async function runDiscordStartupDiagnostics(token) {
    const withTimeout = (promise, ms, label) => Promise.race([
        promise,
        new Promise((_, reject) => setTimeout(() => reject(new Error(`${label} timed out after ${ms}ms`)), ms))
    ]);

    try {
        const gatewayResponse = await withTimeout(
            fetch("https://discord.com/api/v10/gateway"),
            10000,
            "Discord gateway HTTP check"
        );
        console.log(`Discord gateway HTTP check: ${gatewayResponse.status}`);
    } catch (error) {
        console.error("Discord gateway HTTP check failed:", error.message);
    }

    try {
        const authResponse = await withTimeout(
            fetch("https://discord.com/api/v10/users/@me", {
                headers: { Authorization: `Bot ${token}` }
            }),
            10000,
            "Discord token check"
        );
        console.log(`Discord token check: ${authResponse.status}`);
    } catch (error) {
        console.error("Discord token check failed:", error.message);
    }

    await new Promise(resolve => {
        let settled = false;
        const finish = message => {
            if (settled) return;
            settled = true;
            console.log(message);
            resolve();
        };

        let WebSocket;
        try {
            WebSocket = require("ws");
        } catch (error) {
            finish(`Discord WebSocket diagnostic unavailable: ${error.message}`);
            return;
        }

        const socket = new WebSocket("wss://gateway.discord.gg/?v=10&encoding=json");
        const timer = setTimeout(() => {
            socket.terminate();
            finish("Discord WebSocket diagnostic: timeout");
        }, 15000);

        socket.once("open", () => console.log("Discord WebSocket diagnostic: connected"));
        socket.once("message", data => {
            clearTimeout(timer);
            let opcode = "unknown";
            try {
                opcode = JSON.parse(String(data)).op;
            } catch {
                // Keep the diagnostic safe and concise if Discord sends invalid data.
            }
            socket.close();
            finish(`Discord WebSocket diagnostic: received gateway packet (op ${opcode})`);
        });
        socket.once("error", error => {
            clearTimeout(timer);
            finish(`Discord WebSocket diagnostic failed: ${error.message}`);
        });
        socket.once("close", (code, reason) => {
            if (!settled) {
                clearTimeout(timer);
                finish(`Discord WebSocket diagnostic closed: ${code}${reason ? ` (${String(reason)})` : ""}`);
            }
        });
    });
}

async function startDiscordClient() {
    const token = process.env.DISCORD_TOKEN;
    if (!token) {
        console.error("DISCORD_TOKEN is not set. Refusing to run a disconnected bot process.");
        process.exit(1);
    }

    try {
        await runDiscordStartupDiagnostics(token);
        await Promise.race([
            client.login(token),
            new Promise((_, reject) => {
                setTimeout(() => reject(new Error("Discord gateway connection timed out after 60 seconds")), 60000);
            })
        ]);
    } catch (error) {
        console.error("Discord login failed:", error);
        process.exit(1);
    }
}

startDiscordClient();

