param(
    [string]$ResourceGroup = "dca-bot-rg",
    [string]$Location = "centralindia",
    [string]$PlanName = "dca-bot-plan",
    [string]$DashboardAppName = "",
    [string]$BotAppName = "",
    [string]$PostgresServerName = "",
    [string]$DatabaseName = "dcabot",
    [string]$PostgresAdminUser = "dcaadmin",
    [string]$DatabaseUrl = "",
    [string]$SubscriptionId = "",
    [string]$EnvPath = "bot/.env",
    [string]$DiscordClientSecret = "",
    [string]$DashboardAllowedRoleId = "",
    [switch]$AutoGeneratePostgresPassword,
    [switch]$SkipPostgres,
    [switch]$SkipDashboardBuild,
    [switch]$InstallAzureCli
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Test-CommandExists {
    param([string]$Name)
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Import-DotEnv {
    param([string]$Path)

    $values = @{}
    if (-not $Path) { return $values }

    $resolved = if ([IO.Path]::IsPathRooted($Path)) { $Path } else { Join-Path (Resolve-RepoRoot) $Path }
    if (-not (Test-Path -LiteralPath $resolved)) { return $values }

    foreach ($line in Get-Content -LiteralPath $resolved) {
        if ($line -notmatch '^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$') { continue }

        $key = $matches[1]
        $value = $matches[2].Trim()
        if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
            $value = $value.Substring(1, $value.Length - 2)
        }

        $values[$key] = $value
    }

    return $values
}

function Get-ConfigValue {
    param(
        [hashtable]$Values,
        [string]$Name,
        [string]$Fallback = ""
    )

    if ($Values.ContainsKey($Name) -and $Values[$Name]) { return $Values[$Name] }
    $envValue = [Environment]::GetEnvironmentVariable($Name)
    if ($envValue) { return $envValue }
    return $Fallback
}

function Read-RequiredValue {
    param(
        [string]$Prompt,
        [string]$Current = ""
    )

    if ($Current) { return $Current }

    do {
        $value = Read-Host $Prompt
    } while (-not $value)

    return $value
}

function Read-OptionalValue {
    param(
        [string]$Prompt,
        [string]$Current = ""
    )

    if ($Current) { return $Current }
    return Read-Host $Prompt
}

function Read-SecretPlainText {
    param(
        [string]$Prompt,
        [string]$Current = ""
    )

    if ($Current) { return $Current }

    do {
        $secure = Read-Host $Prompt -AsSecureString
    } while ($secure.Length -eq 0)

    $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)
    } finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
    }
}

function New-RandomSuffix {
    return (Get-Random -Minimum 10000 -Maximum 99999).ToString()
}

function New-SessionSecret {
    return ([guid]::NewGuid().ToString("N") + [guid]::NewGuid().ToString("N"))
}

function New-PostgresPassword {
    $bytes = New-Object byte[] 18
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }

    $base = [Convert]::ToBase64String($bytes).TrimEnd("=") -replace "[+/]", "9"
    return "Pg1!$base"
}

function Invoke-Az {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)

    $attempt = 0
    do {
        $attempt++
        & az @Arguments --only-show-errors -o none
        $exitCode = $LASTEXITCODE

        if ($exitCode -eq 0) { return }

        if ($attempt -lt 4) {
            Write-Host "Azure CLI command failed; retrying in $($attempt * 10) seconds..."
            Start-Sleep -Seconds ($attempt * 10)
        }
    } while ($attempt -lt 4)

    if ($exitCode -ne 0) {
        throw "az $($Arguments -join ' ') failed with exit code $exitCode"
    }
}

function Invoke-AzJson {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)

    $json = & az @Arguments -o json
    if ($LASTEXITCODE -ne 0) {
        throw "az $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
    if (-not $json) { return $null }
    return $json | ConvertFrom-Json
}

function Invoke-AzOptionalJson {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)

    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"

    try {
        $json = & az @Arguments -o json 2>$null
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }

    if ($exitCode -ne 0 -or -not $json) { return $null }
    return $json | ConvertFrom-Json
}

function Ensure-AzureCli {
    if (Test-CommandExists "az") { return }

    $knownPaths = @(
        "C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin",
        "C:\Program Files (x86)\Microsoft SDKs\Azure\CLI2\wbin",
        "$env:LOCALAPPDATA\Programs\Microsoft SDKs\Azure\CLI2\wbin"
    )

    foreach ($knownPath in $knownPaths) {
        if (Test-Path (Join-Path $knownPath "az.cmd")) {
            $env:Path = "$knownPath;$env:Path"
            if (Test-CommandExists "az") { return }
        }
    }

    if ($InstallAzureCli) {
        if (-not (Test-CommandExists "winget")) {
            throw "Azure CLI is missing and winget is not available. Install Azure CLI, then rerun this script."
        }

        Write-Host "Installing Azure CLI with winget..."
        winget install --id Microsoft.AzureCLI -e --accept-package-agreements --accept-source-agreements
        if ($LASTEXITCODE -ne 0) {
            throw "Azure CLI install failed. Install it manually, then rerun this script."
        }
    } else {
        throw "Azure CLI is missing. Rerun with -InstallAzureCli or install it from https://learn.microsoft.com/cli/azure/install-azure-cli-windows"
    }
}

function Ensure-AzureLogin {
    $account = Invoke-AzOptionalJson account show
    if (-not $account) {
        Write-Host "Opening Azure login..."
        Invoke-Az login
    }

    if ($SubscriptionId) {
        Invoke-Az account set --subscription $SubscriptionId
    }

    $current = Invoke-AzJson account show
    Write-Host "Using Azure subscription: $($current.name) ($($current.id))"
}

function Ensure-WebApp {
    param(
        [string]$Name,
        [string]$ResourceGroupName,
        [string]$Plan
    )

    $existing = Invoke-AzOptionalJson webapp show --resource-group $ResourceGroupName --name $Name
    if ($existing) {
        Write-Host "Web App exists: $Name"
        return
    }

    Write-Host "Creating Web App: $Name"
    Invoke-Az webapp create `
        --resource-group $ResourceGroupName `
        --plan $Plan `
        --name $Name `
        --runtime "NODE:22-lts"
}

function Set-AppSettings {
    param(
        [string]$AppName,
        [string]$ResourceGroupName,
        [hashtable]$Settings
    )

    $settingArgs = @()
    foreach ($entry in $Settings.GetEnumerator()) {
        if ($null -eq $entry.Value -or [string]$entry.Value -eq "") { continue }
        $settingArgs += "$($entry.Key)=$($entry.Value)"
    }

    if ($settingArgs.Count -eq 0) { return }

    Invoke-Az webapp config appsettings set `
        --resource-group $ResourceGroupName `
        --name $AppName `
        --settings @settingArgs
}

function Copy-AppPackage {
    param(
        [string]$SourceDir,
        [string]$StageDir,
        [string[]]$ExcludeNames
    )

    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

    Get-ChildItem -LiteralPath $SourceDir -Force | Where-Object {
        $ExcludeNames -notcontains $_.Name
    } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $StageDir $_.Name) -Recurse -Force
    }
}

function New-AppZip {
    param(
        [string]$SourceDir,
        [string]$ZipPath,
        [string[]]$ExcludeNames
    )

    $repoRoot = Resolve-RepoRoot
    $deployRoot = Join-Path $repoRoot ".azure-deploy"
    $stageDir = Join-Path $deployRoot ([IO.Path]::GetFileNameWithoutExtension($ZipPath))

    $resolvedDeployRoot = [IO.Path]::GetFullPath($deployRoot)
    $resolvedStage = [IO.Path]::GetFullPath($stageDir)
    if (-not $resolvedStage.StartsWith($resolvedDeployRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clean an unexpected staging path: $resolvedStage"
    }

    if (Test-Path -LiteralPath $stageDir) {
        Remove-Item -LiteralPath $stageDir -Recurse -Force
    }
    if (Test-Path -LiteralPath $ZipPath) {
        Remove-Item -LiteralPath $ZipPath -Force
    }

    Copy-AppPackage -SourceDir $SourceDir -StageDir $stageDir -ExcludeNames $ExcludeNames

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::Open($ZipPath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        Get-ChildItem -LiteralPath $stageDir -Recurse -File | ForEach-Object {
            $relativePath = $_.FullName.Substring($stageDir.Length).TrimStart("\", "/") -replace "\\", "/"
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zip,
                $_.FullName,
                $relativePath,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
    } finally {
        $zip.Dispose()
    }
}

$repoRoot = Resolve-RepoRoot
$deployRoot = Join-Path $repoRoot ".azure-deploy"
New-Item -ItemType Directory -Force -Path $deployRoot | Out-Null

$envValues = Import-DotEnv -Path $EnvPath

Ensure-AzureCli
Ensure-AzureLogin

$suffix = New-RandomSuffix
if (-not $DashboardAppName) { $DashboardAppName = "dca-dashboard-$suffix" }
if (-not $BotAppName) { $BotAppName = "dca-bot-$suffix" }
if (-not $PostgresServerName) { $PostgresServerName = "dca-pg-$suffix" }

$dashboardUrl = "https://$DashboardAppName.azurewebsites.net"
$redirectUri = "$dashboardUrl/auth/discord/callback"

$discordToken = Read-SecretPlainText "Discord bot token" (Get-ConfigValue -Values $envValues -Name "DISCORD_TOKEN")
$discordClientId = Read-RequiredValue "Discord application/client ID" (Get-ConfigValue -Values $envValues -Name "DISCORD_CLIENT_ID")
$discordClientSecret = Read-SecretPlainText "Discord OAuth client secret" (Get-ConfigValue -Values $envValues -Name "DISCORD_CLIENT_SECRET" -Fallback $DiscordClientSecret)
$discordGuildId = Read-RequiredValue "Fallback Discord guild/server ID" (Get-ConfigValue -Values $envValues -Name "DISCORD_GUILD_ID")
$communityGuildId = Get-ConfigValue -Values $envValues -Name "COMMUNITY_GUILD_ID"
$recruitmentGuildId = Get-ConfigValue -Values $envValues -Name "RECRUITMENT_GUILD_ID"
$dashboardRoleId = Read-RequiredValue "Dashboard allowed role ID" (Get-ConfigValue -Values $envValues -Name "DASHBOARD_ALLOWED_ROLE_ID" -Fallback $DashboardAllowedRoleId)
$recruiterRoleId = Get-ConfigValue -Values $envValues -Name "RECRUITER_ROLE_ID"
$geminiApiKey = Get-ConfigValue -Values $envValues -Name "GEMINI_API_KEY"
$dashboardSessionSecret = New-SessionSecret

if (-not $communityGuildId) { $communityGuildId = $discordGuildId }
if (-not $recruitmentGuildId) { $recruitmentGuildId = $discordGuildId }

Write-Host "Creating resource group: $ResourceGroup"
Invoke-Az group create --name $ResourceGroup --location $Location

if (-not $DatabaseUrl -and -not $SkipPostgres) {
    $postgresPassword = if ($AutoGeneratePostgresPassword) {
        New-PostgresPassword
    } else {
        Read-SecretPlainText "New Azure Postgres admin password"
    }

    Write-Host "Creating or updating Azure Postgres Flexible Server: $PostgresServerName"
    $pgExists = Invoke-AzOptionalJson postgres flexible-server show --resource-group $ResourceGroup --name $PostgresServerName
    if (-not $pgExists) {
        Invoke-Az postgres flexible-server create `
            --resource-group $ResourceGroup `
            --name $PostgresServerName `
            --location $Location `
            --admin-user $PostgresAdminUser `
            --admin-password $postgresPassword `
            --sku-name Standard_B1ms `
            --tier Burstable `
            --storage-size 32 `
            --version 16 `
            --public-access 0.0.0.0
    } else {
        Invoke-Az postgres flexible-server update `
            --resource-group $ResourceGroup `
            --name $PostgresServerName `
            --admin-password $postgresPassword
    }

    $firewallRule = Invoke-AzOptionalJson postgres flexible-server firewall-rule show --resource-group $ResourceGroup --name $PostgresServerName --rule-name AllowAzureServices
    if (-not $firewallRule) {
        Invoke-Az postgres flexible-server firewall-rule create `
            --resource-group $ResourceGroup `
            --name $PostgresServerName `
            --rule-name AllowAzureServices `
            --start-ip-address 0.0.0.0 `
            --end-ip-address 0.0.0.0
    }

    $dbExists = Invoke-AzOptionalJson postgres flexible-server db show --resource-group $ResourceGroup --server-name $PostgresServerName --database-name $DatabaseName
    if (-not $dbExists) {
        Invoke-Az postgres flexible-server db create `
            --resource-group $ResourceGroup `
            --server-name $PostgresServerName `
            --database-name $DatabaseName
    }

    $encodedUser = [uri]::EscapeDataString($PostgresAdminUser)
    $encodedPassword = [uri]::EscapeDataString($postgresPassword)
    $DatabaseUrl = "postgres://$encodedUser`:$encodedPassword@$PostgresServerName.postgres.database.azure.com:5432/$DatabaseName`?sslmode=require"
}

if (-not $DatabaseUrl) {
    $DatabaseUrl = Read-RequiredValue "DATABASE_URL"
}

Write-Host "Creating App Service plan: $PlanName"
$planExists = Invoke-AzOptionalJson appservice plan show --resource-group $ResourceGroup --name $PlanName
if (-not $planExists) {
    Invoke-Az appservice plan create `
        --resource-group $ResourceGroup `
        --name $PlanName `
        --location $Location `
        --is-linux `
        --sku B1
}

Ensure-WebApp -Name $BotAppName -ResourceGroupName $ResourceGroup -Plan $PlanName
Ensure-WebApp -Name $DashboardAppName -ResourceGroupName $ResourceGroup -Plan $PlanName

Write-Host "Configuring App Service settings..."
Invoke-Az webapp config set --resource-group $ResourceGroup --name $BotAppName --always-on true
Invoke-Az webapp config set --resource-group $ResourceGroup --name $DashboardAppName --always-on true
Invoke-Az webapp config set --resource-group $ResourceGroup --name $BotAppName --startup-file "npm start"
Invoke-Az webapp config set --resource-group $ResourceGroup --name $DashboardAppName --startup-file "npm start"

Set-AppSettings -ResourceGroupName $ResourceGroup -AppName $BotAppName -Settings @{
    SCM_DO_BUILD_DURING_DEPLOYMENT = "true"
    WEBSITE_NODE_DEFAULT_VERSION = "~22"
    DISCORD_TOKEN = $discordToken
    DISCORD_CLIENT_ID = $discordClientId
    DATABASE_URL = $DatabaseUrl
    DATABASE_SSL = "true"
    DISCORD_GUILD_ID = $discordGuildId
    COMMUNITY_GUILD_ID = $communityGuildId
    RECRUITMENT_GUILD_ID = $recruitmentGuildId
    RECRUITER_ROLE_ID = $recruiterRoleId
    DASHBOARD_BASE_URL = $dashboardUrl
    GEMINI_API_KEY = $geminiApiKey
}

Set-AppSettings -ResourceGroupName $ResourceGroup -AppName $DashboardAppName -Settings @{
    SCM_DO_BUILD_DURING_DEPLOYMENT = "true"
    WEBSITE_NODE_DEFAULT_VERSION = "~22"
    DISCORD_TOKEN = $discordToken
    DISCORD_CLIENT_ID = $discordClientId
    DISCORD_CLIENT_SECRET = $discordClientSecret
    DATABASE_URL = $DatabaseUrl
    DATABASE_SSL = "true"
    DASHBOARD_BASE_URL = $dashboardUrl
    DISCORD_REDIRECT_URI = $redirectUri
    DASHBOARD_SESSION_SECRET = $dashboardSessionSecret
    DISCORD_GUILD_ID = $discordGuildId
    COMMUNITY_GUILD_ID = $communityGuildId
    RECRUITMENT_GUILD_ID = $recruitmentGuildId
    DASHBOARD_ALLOWED_ROLE_ID = $dashboardRoleId
    RECRUITER_ROLE_ID = $recruiterRoleId
    GEMINI_API_KEY = $geminiApiKey
}

if (-not $SkipDashboardBuild) {
    Write-Host "Installing and building dashboard..."
    Push-Location (Join-Path $repoRoot "dashboard")
    try {
        npm install
        if ($LASTEXITCODE -ne 0) { throw "npm install failed for dashboard" }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "npm run build failed for dashboard" }
    } finally {
        Pop-Location
    }
}

$botZip = Join-Path $deployRoot "bot.zip"
$dashboardZip = Join-Path $deployRoot "dashboard.zip"

Write-Host "Creating deployment packages..."
New-AppZip `
    -SourceDir (Join-Path $repoRoot "bot") `
    -ZipPath $botZip `
    -ExcludeNames @("node_modules", ".env", "data")

New-AppZip `
    -SourceDir (Join-Path $repoRoot "dashboard") `
    -ZipPath $dashboardZip `
    -ExcludeNames @("node_modules", ".env", ".vite")

Write-Host "Deploying bot..."
Invoke-Az webapp deploy `
    --resource-group $ResourceGroup `
    --name $BotAppName `
    --src-path $botZip `
    --type zip

Write-Host "Deploying dashboard..."
Invoke-Az webapp deploy `
    --resource-group $ResourceGroup `
    --name $DashboardAppName `
    --src-path $dashboardZip `
    --type zip

Write-Host ""
Write-Host "Deployment submitted."
Write-Host "Dashboard: $dashboardUrl/dashboard"
Write-Host "Bot health: https://$BotAppName.azurewebsites.net/health"
Write-Host ""
Write-Host "Required final manual step:"
Write-Host "Add this Discord OAuth redirect URI in the Discord Developer Portal:"
Write-Host $redirectUri
