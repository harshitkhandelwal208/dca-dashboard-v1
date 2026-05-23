const { defineConfig } = require("vite");
const react = require("@vitejs/plugin-react");

module.exports = defineConfig({
    plugins: [react()],
    server: {
        port: 5173,
        proxy: {
            "/api": "http://localhost:3000",
            "/auth": "http://localhost:3000",
            "/uploads": "http://localhost:3000"
        }
    },
    build: {
        outDir: "dist",
        emptyOutDir: true
    }
});
