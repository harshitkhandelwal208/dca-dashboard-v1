//! Self-contained HTML/CSS rendering for the DCA Web Dashboard.

use dca_state::config::DashboardConfig;
use dca_state::log_store::AuditLogEntry;
use dca_state::recruitment_store::Ticket;
use dca_state::spreadsheet_store::SpreadsheetSession;

pub fn render_dashboard_html(
    uptime_secs: u64,
    config: &DashboardConfig,
    tickets: &[Ticket],
    sessions: &[SpreadsheetSession],
    logs: &[AuditLogEntry],
) -> String {
    let uptime_hours = uptime_secs / 3600;
    let uptime_mins = (uptime_secs % 3600) / 60;
    let uptime_str = format!("{}h {}m", uptime_hours, uptime_mins);

    let open_tickets_count = tickets.iter().filter(|t| t.status == "open" || t.status == "claimed").count();
    let completed_sessions_count = sessions.iter().filter(|s| s.status == "completed").count();
    let total_logs_count = logs.len();

    // Render tickets rows
    let mut tickets_rows = String::new();
    for t in tickets.iter().rev().take(50) {
        let status_color = match t.status.as_str() {
            "open" => "bg-emerald-950 text-emerald-300 border-emerald-800",
            "claimed" => "bg-sky-950 text-sky-300 border-sky-800",
            "closed" => "bg-purple-950 text-purple-300 border-purple-800",
            _ => "bg-slate-800 text-slate-300 border-slate-700",
        };

        let player_name = t.license_analysis.as_ref()
            .and_then(|v| v.get("player_name").or_else(|| v.get("playerName")))
            .and_then(|n| n.as_str())
            .unwrap_or("—");

        let gp = t.license_analysis.as_ref()
            .and_then(|v| v.get("garage_power").or_else(|| v.get("garagePower")))
            .and_then(|g| g.as_u64())
            .map(|g| format!("{} GP", g))
            .unwrap_or_else(|| "—".to_string());

        let team = t.assigned_team.as_deref().unwrap_or("—");
        let date_str = t.created_at.chars().take(10).collect::<String>();

        tickets_rows.push_str(&format!(
            r#"<tr>
                <td class="font-mono text-teal-400">#{}</td>
                <td class="font-medium text-slate-200">{}</td>
                <td class="text-slate-300">{}</td>
                <td class="font-mono text-slate-300">{}</td>
                <td><span class="px-2 py-0.5 text-xs font-semibold rounded border {}">{}</span></td>
                <td class="text-slate-300">{}</td>
                <td class="text-xs text-slate-400 font-mono">{}</td>
            </tr>"#,
            t.id, t.applicant_tag, player_name, gp, status_color, t.status, team, date_str
        ));
    }
    if tickets_rows.is_empty() {
        tickets_rows = r#"<tr><td colspan="7" class="py-6 text-center text-slate-400">No recruitment tickets recorded yet.</td></tr>"#.to_string();
    }

    // Render spreadsheet sessions rows
    let mut sessions_rows = String::new();
    for s in sessions.iter().rev().take(50) {
        let status_color = match s.status.as_str() {
            "completed" => "bg-teal-950 text-teal-300 border-teal-800",
            "processing" => "bg-amber-950 text-amber-300 border-amber-800",
            "pending" => "bg-sky-950 text-sky-300 border-sky-800",
            _ => "bg-rose-950 text-rose-300 border-rose-800",
        };

        let date_str = s.created_at.chars().take(10).collect::<String>();
        let xlsx_link = if s.status == "completed" {
            format!(r#"<a href="/spreadsheets/{}.xlsx" class="px-2.5 py-1 text-xs font-medium rounded bg-teal-800 hover:bg-teal-700 text-teal-100 transition inline-block">Download XLSX</a>"#, s.id)
        } else {
            "—".to_string()
        };

        sessions_rows.push_str(&format!(
            r#"<tr>
                <td class="font-mono text-teal-400">{}</td>
                <td class="font-medium text-slate-200">{}</td>
                <td><span class="px-2 py-0.5 text-xs font-semibold rounded border {}">{}</span></td>
                <td class="text-center font-mono text-slate-300">{}</td>
                <td class="text-slate-300 text-xs font-mono">{}</td>
                <td class="text-slate-400 text-xs font-mono">{}</td>
                <td>{}</td>
            </tr>"#,
            s.id, s.team_name, status_color, s.status, s.images.len(), s.author_tag, date_str, xlsx_link
        ));
    }
    if sessions_rows.is_empty() {
        sessions_rows = r#"<tr><td colspan="7" class="py-6 text-center text-slate-400">No spreadsheet sessions recorded yet.</td></tr>"#.to_string();
    }

    // Render audit logs rows
    let mut logs_rows = String::new();
    for l in logs.iter().rev().take(60) {
        let date_str = l.timestamp.chars().take(19).collect::<String>().replace('T', " ");
        logs_rows.push_str(&format!(
            r#"<tr>
                <td class="text-xs font-mono text-slate-400">{}</td>
                <td class="font-mono font-semibold text-teal-400">{}</td>
                <td class="text-slate-300 font-medium">{}</td>
                <td class="text-slate-300 text-sm">{}</td>
            </tr>"#,
            date_str, l.action, l.actor_tag, l.details
        ));
    }
    if logs_rows.is_empty() {
        logs_rows = r#"<tr><td colspan="4" class="py-6 text-center text-slate-400">No audit log entries recorded yet.</td></tr>"#.to_string();
    }

    // Config JSON formatted
    let config_json = serde_json::to_string_pretty(config).unwrap_or_default();

    format!(r#"<!DOCTYPE html>
<html lang="en" class="dark">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>DCA Control Center • Pure Rust Edition</title>
    <style>
        *, ::before, ::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            background-color: #0b1120;
            color: #f1f5f9;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
        }}
        .header {{
            background: #0f172a;
            border-bottom: 1px solid #1e293b;
            padding: 1rem 2rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .brand {{
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }}
        .brand h1 {{
            font-size: 1.25rem;
            font-weight: 700;
            letter-spacing: -0.02em;
            color: #f8fafc;
        }}
        .brand-tag {{
            background: #0f766e;
            color: #ccfbf1;
            font-size: 0.7rem;
            font-weight: 600;
            padding: 0.2rem 0.5rem;
            border-radius: 9999px;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }}
        .header-meta {{
            display: flex;
            align-items: center;
            gap: 1.5rem;
            font-size: 0.85rem;
            color: #94a3b8;
        }}
        .status-dot {{
            width: 8px;
            height: 8px;
            background-color: #10b981;
            border-radius: 50%;
            display: inline-block;
            box-shadow: 0 0 8px #10b981;
            margin-right: 0.35rem;
        }}
        .main-container {{
            max-width: 1400px;
            width: 100%;
            margin: 0 auto;
            padding: 2rem;
            flex: 1;
        }}
        .grid-kpi {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
            gap: 1.25rem;
            margin-bottom: 2rem;
        }}
        .kpi-card {{
            background: #1e293b;
            border: 1px solid #334155;
            border-radius: 0.75rem;
            padding: 1.25rem;
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }}
        .kpi-label {{
            font-size: 0.8rem;
            text-transform: uppercase;
            font-weight: 600;
            letter-spacing: 0.05em;
            color: #94a3b8;
        }}
        .kpi-val {{
            font-size: 2rem;
            font-weight: 800;
            color: #f8fafc;
            line-height: 1;
        }}
        .kpi-sub {{
            font-size: 0.75rem;
            color: #14b8a6;
        }}
        /* Tab Navigation via Pure CSS Radio Elements */
        .tab-input {{
            display: none;
        }}
        .nav-tabs {{
            display: flex;
            gap: 0.5rem;
            border-bottom: 1px solid #334155;
            margin-bottom: 1.5rem;
        }}
        .nav-label {{
            padding: 0.75rem 1.25rem;
            font-size: 0.9rem;
            font-weight: 600;
            color: #94a3b8;
            cursor: pointer;
            border-bottom: 2px solid transparent;
            transition: all 0.15s ease;
        }}
        .nav-label:hover {{
            color: #f1f5f9;
        }}
        #tab-tickets-input:checked ~ .nav-tabs label[for="tab-tickets-input"],
        #tab-spreadsheets-input:checked ~ .nav-tabs label[for="tab-spreadsheets-input"],
        #tab-logs-input:checked ~ .nav-tabs label[for="tab-logs-input"],
        #tab-config-input:checked ~ .nav-tabs label[for="tab-config-input"] {{
            color: #14b8a6;
            border-bottom-color: #14b8a6;
        }}
        .tab-content {{
            display: none;
        }}
        #tab-tickets-input:checked ~ #tab-tickets-content {{
            display: block;
        }}
        #tab-spreadsheets-input:checked ~ #tab-spreadsheets-content {{
            display: block;
        }}
        #tab-logs-input:checked ~ #tab-logs-content {{
            display: block;
        }}
        #tab-config-input:checked ~ #tab-config-content {{
            display: block;
        }}
        .content-card {{
            background: #1e293b;
            border: 1px solid #334155;
            border-radius: 0.75rem;
            overflow: hidden;
        }}
        .table-responsive {{
            width: 100%;
            overflow-x: auto;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            text-align: left;
            font-size: 0.875rem;
        }}
        thead {{
            background: #0f172a;
            border-bottom: 1px solid #334155;
        }}
        th {{
            padding: 0.85rem 1.25rem;
            font-weight: 600;
            color: #94a3b8;
            font-size: 0.75rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }}
        td {{
            padding: 1rem 1.25rem;
            border-bottom: 1px solid #1e293b;
        }}
        tr:hover td {{
            background: rgba(15, 23, 42, 0.4);
        }}
        .font-mono {{ font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }}
        .text-teal-400 {{ color: #2dd4bf; }}
        .text-slate-200 {{ color: #e2e8f0; }}
        .text-slate-300 {{ color: #cbd5e1; }}
        .text-slate-400 {{ color: #94a3b8; }}
        .bg-emerald-950 {{ background: #064e3b; }}
        .text-emerald-300 {{ color: #6ee7b7; }}
        .border-emerald-800 {{ border: 1px solid #065f46; }}
        .bg-sky-950 {{ background: #082f49; }}
        .text-sky-300 {{ color: #7dd3fc; }}
        .border-sky-800 {{ border: 1px solid #075985; }}
        .bg-purple-950 {{ background: #3b0764; }}
        .text-purple-300 {{ color: #d8b4fe; }}
        .border-purple-800 {{ border: 1px solid #581c87; }}
        .bg-teal-950 {{ background: #042f2e; }}
        .text-teal-300 {{ color: #5eead4; }}
        .border-teal-800 {{ border: 1px solid #115e59; }}
        .bg-rose-950 {{ background: #4c0519; }}
        .text-rose-300 {{ color: #fda4af; }}
        .border-rose-800 {{ border: 1px solid #881337; }}
        .bg-amber-950 {{ background: #451a03; }}
        .text-amber-300 {{ color: #fcd34d; }}
        .border-amber-800 {{ border: 1px solid #78350f; }}
        .bg-teal-800 {{ background: #115e59; }}
        .hover\:bg-teal-700:hover {{ background: #0f766e; }}
        .text-teal-100 {{ color: #ccfbf1; }}
        .rounded {{ border-radius: 0.375rem; }}
        .px-2 {{ padding-left: 0.5rem; padding-right: 0.5rem; }}
        .py-0\.5 {{ padding-top: 0.125rem; padding-bottom: 0.125rem; }}
        .px-2\.5 {{ padding-left: 0.625rem; padding-right: 0.625rem; }}
        .py-1 {{ padding-top: 0.25rem; padding-bottom: 0.25rem; }}
        .text-xs {{ font-size: 0.75rem; }}
        .text-sm {{ font-size: 0.875rem; }}
        .font-semibold {{ font-weight: 600; }}
        .font-medium {{ font-weight: 500; }}
        .text-center {{ text-align: center; }}
        .transition {{ transition: all 0.15s ease; }}
        .inline-block {{ display: inline-block; }}
        .footer {{
            padding: 1.5rem 2rem;
            text-align: center;
            font-size: 0.8rem;
            color: #64748b;
            border-top: 1px solid #1e293b;
        }}
        pre.code-block {{
            background: #090d16;
            color: #cbd5e1;
            padding: 1.5rem;
            border-radius: 0.5rem;
            font-size: 0.85rem;
            overflow-x: auto;
            border: 1px solid #334155;
            line-height: 1.5;
        }}
    </style>
</head>
<body>
    <header class="header">
        <div class="brand">
            <h1>DCA Control Center</h1>
            <span class="brand-tag">Pure Rust Edition</span>
        </div>
        <div class="header-meta">
            <div><span class="status-dot"></span>Discord Connected</div>
            <div>Uptime: <strong style="color:#f8fafc">{}</strong></div>
            <div>Engine: <strong style="color:#2dd4bf">dca-bot (native)</strong></div>
        </div>
    </header>

    <main class="main-container">
        <!-- Top Metrics Cards -->
        <div class="grid-kpi">
            <div class="kpi-card">
                <span class="kpi-label">Active Tickets</span>
                <span class="kpi-val">{}</span>
                <span class="kpi-sub">{} total recorded</span>
            </div>
            <div class="kpi-card">
                <span class="kpi-label">Completed Spreadsheets</span>
                <span class="kpi-val">{}</span>
                <span class="kpi-sub">{} total sessions</span>
            </div>
            <div class="kpi-card">
                <span class="kpi-label">Logged Audit Actions</span>
                <span class="kpi-val">{}</span>
                <span class="kpi-sub">Persistent audit trail</span>
            </div>
            <div class="kpi-card">
                <span class="kpi-label">VM Resource Profile</span>
                <span class="kpi-val" style="font-size:1.5rem; color:#2dd4bf">~18 MB RAM</span>
                <span class="kpi-sub">Render Free Tier Optimized (0% Node)</span>
            </div>
        </div>

        <!-- Navigation Tabs -->
        <input type="radio" name="dash-tab" id="tab-tickets-input" class="tab-input" checked>
        <input type="radio" name="dash-tab" id="tab-spreadsheets-input" class="tab-input">
        <input type="radio" name="dash-tab" id="tab-logs-input" class="tab-input">
        <input type="radio" name="dash-tab" id="tab-config-input" class="tab-input">

        <nav class="nav-tabs">
            <label for="tab-tickets-input" class="nav-label">Recruitment Tickets</label>
            <label for="tab-spreadsheets-input" class="nav-label">Spreadsheets & Standings</label>
            <label for="tab-logs-input" class="nav-label">System Audit Logs</label>
            <label for="tab-config-input" class="nav-label">Dashboard Config</label>
        </nav>

        <!-- Recruitment Content -->
        <section id="tab-tickets-content" class="tab-content">
            <div class="content-card">
                <div class="table-responsive">
                    <table>
                        <thead>
                            <tr>
                                <th>Ticket ID</th>
                                <th>Applicant</th>
                                <th>In-Game Name</th>
                                <th>Garage Power</th>
                                <th>Status</th>
                                <th>Assigned Team</th>
                                <th>Created At</th>
                            </tr>
                        </thead>
                        <tbody>
                            {}
                        </tbody>
                    </table>
                </div>
            </div>
        </section>

        <!-- Spreadsheets Content -->
        <section id="tab-spreadsheets-content" class="tab-content">
            <div class="content-card">
                <div class="table-responsive">
                    <table>
                        <thead>
                            <tr>
                                <th>Session ID</th>
                                <th>Team</th>
                                <th>Status</th>
                                <th>Screenshots</th>
                                <th>Author</th>
                                <th>Created At</th>
                                <th>Action</th>
                            </tr>
                        </thead>
                        <tbody>
                            {}
                        </tbody>
                    </table>
                </div>
            </div>
        </section>

        <!-- Audit Logs Content -->
        <section id="tab-logs-content" class="tab-content">
            <div class="content-card">
                <div class="table-responsive">
                    <table>
                        <thead>
                            <tr>
                                <th>Timestamp</th>
                                <th>Action</th>
                                <th>Actor</th>
                                <th>Details</th>
                            </tr>
                        </thead>
                        <tbody>
                            {}
                        </tbody>
                    </table>
                </div>
            </div>
        </section>

        <!-- Config Content -->
        <section id="tab-config-content" class="tab-content">
            <div class="content-card" style="padding: 1.5rem;">
                <h2 style="font-size: 1.1rem; font-weight: 600; margin-bottom: 1rem; color: #f8fafc;">Live Dashboard Configuration</h2>
                <pre class="code-block">{}</pre>
            </div>
        </section>
    </main>

    <footer class="footer">
        DCA Discord Automation • Pure Rust Server • 100% Native Tesseract 5 & XLSX Architecture
    </footer>
</body>
</html>"#,
        uptime_str,
        open_tickets_count, tickets.len(),
        completed_sessions_count, sessions.len(),
        total_logs_count,
        tickets_rows,
        sessions_rows,
        logs_rows,
        config_json
    )
}
