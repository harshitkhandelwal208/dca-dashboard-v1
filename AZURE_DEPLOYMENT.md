# Azure Deployment

This repo deploys as two Azure App Services in one Linux App Service plan:

- `bot/` runs the Discord bot runtime.
- `dashboard/` runs the Express dashboard and serves the built React app.

Both apps must share the same `DATABASE_URL`. The app supports PostgreSQL URLs and Azure SQL
connection strings; this deployment uses Azure SQL Database's free offer with `DATABASE_CLIENT=sqlserver`.

## Least-Work Path

Run this from the repo root in PowerShell:

```powershell
.\scripts\deploy-azure.ps1 -InstallAzureCli
```

The script will:

1. Install/check Azure CLI.
2. Open Azure login if needed.
3. Create a resource group, Linux App Service plan, two web apps, and an optional database.
4. Prompt for Discord/Gemini secrets.
5. Build the dashboard.
6. Zip-deploy both apps.

After it finishes, add the printed redirect URI to the Discord Developer Portal OAuth2 redirect list.

## Useful Options

Use an existing database:

```powershell
.\scripts\deploy-azure.ps1 -SkipPostgres -DatabaseUrl "postgres://..."
```

For Azure SQL, use an ADO.NET-style connection string and set `DATABASE_CLIENT=sqlserver` in both App Services.

Pick stable names:

```powershell
.\scripts\deploy-azure.ps1 `
  -ResourceGroup "dca-prod-rg" `
  -PlanName "dca-prod-plan" `
  -BotAppName "dca-prod-bot" `
  -DashboardAppName "dca-prod-dashboard"
```

Pick a subscription:

```powershell
.\scripts\deploy-azure.ps1 -SubscriptionId "<subscription-id>"
```

## Notes

- The default plan is Basic B1 because the Discord bot needs Always On. Free App Service plans can sleep and disconnect the bot.
- Azure SQL Database's free offer must be created with free limits enabled and exhaustion behavior set to auto-pause to avoid overage.
- Azure App Service builds production dependencies during zip deployment.
- The dashboard URL will be `https://<dashboard-app-name>.azurewebsites.net/dashboard`.
- The Discord OAuth redirect URI must exactly match `https://<dashboard-app-name>.azurewebsites.net/auth/discord/callback`.

## Continuous Deployment

This repo includes `.github/workflows/deploy-azure.yml`. Every push to `main` builds the dashboard, packages `bot/` and `dashboard/`, logs in to Azure with GitHub OIDC, and zip-deploys both existing App Services:

- Bot: `dca-bot-31464`
- Dashboard: `dca-dashboard-31464`

The Azure identity is `dca-github-actions-deployer` in `dca-bot-rg`, with a federated credential for `harshitkhandelwal208/dca-dashboard-v1` on `main` and the `Website Contributor` role scoped to `dca-bot-rg`.
