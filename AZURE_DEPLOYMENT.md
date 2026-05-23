# Azure Deployment

This repo deploys as two Azure App Services in one Linux App Service plan:

- `bot/` runs the Discord bot runtime.
- `dashboard/` runs the Express dashboard and serves the built React app.

Both apps must share the same `DATABASE_URL`.

## Least-Work Path

Run this from the repo root in PowerShell:

```powershell
.\scripts\deploy-azure.ps1 -InstallAzureCli
```

The script will:

1. Install/check Azure CLI.
2. Open Azure login if needed.
3. Create a resource group, Linux App Service plan, two web apps, and an optional Azure Postgres Flexible Server.
4. Prompt for Discord/Gemini secrets.
5. Build the dashboard.
6. Zip-deploy both apps.

After it finishes, add the printed redirect URI to the Discord Developer Portal OAuth2 redirect list.

## Useful Options

Use an existing database:

```powershell
.\scripts\deploy-azure.ps1 -SkipPostgres -DatabaseUrl "postgres://..."
```

Pick stable names:

```powershell
.\scripts\deploy-azure.ps1 `
  -ResourceGroup "dca-prod-rg" `
  -PlanName "dca-prod-plan" `
  -BotAppName "dca-prod-bot" `
  -DashboardAppName "dca-prod-dashboard" `
  -PostgresServerName "dca-prod-pg"
```

Pick a subscription:

```powershell
.\scripts\deploy-azure.ps1 -SubscriptionId "<subscription-id>"
```

## Notes

- The default plan is Basic B1 because the Discord bot needs Always On. Free App Service plans can sleep and disconnect the bot.
- Azure App Service builds production dependencies during zip deployment.
- The dashboard URL will be `https://<dashboard-app-name>.azurewebsites.net/dashboard`.
- The Discord OAuth redirect URI must exactly match `https://<dashboard-app-name>.azurewebsites.net/auth/discord/callback`.
