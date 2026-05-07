require("dotenv").config();

const express = require("express");
const { registerDashboardRoutes } = require("./server/dashboardRoutes");

process.on("unhandledRejection", reason => console.error("Unhandled Rejection:", reason));
process.on("uncaughtException", err => console.error("Uncaught Exception:", err));

const app = express();
const PORT = process.env.PORT || 3000;

app.set("trust proxy", 1);
app.use(express.json({ limit: "512kb" }));

registerDashboardRoutes(app);

app.get("/", (req, res) => {
    res.json({
        status: "alive",
        app: "dca-dashboard",
        uptime: Math.floor(process.uptime()),
        timestamp: new Date().toISOString()
    });
});

app.get("/health", (req, res) => {
    res.status(200).json({
        status: "healthy",
        uptime: Math.floor(process.uptime()),
        timestamp: new Date().toISOString()
    });
});

if (require.main === module) {
    app.listen(PORT, () => {
        console.log(`Dashboard server running on ${PORT}`);
    });
}

module.exports = app;
