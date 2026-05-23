import { useEffect, useMemo, useState } from "react";
import {
    Bell,
    Bot,
    BookOpen,
    CheckCircle2,
    ChevronRight,
    ClipboardList,
    Eye,
    ExternalLink,
    FileText,
    FileSpreadsheet,
    Gauge,
    Image,
    ListChecks,
    Loader2,
    Lock,
    MessageSquare,
    Plus,
    Radio,
    RefreshCw,
    Save,
    Search,
    Send,
    Trash2,
    Upload,
    Users,
    Video,
    X
} from "lucide-react";

const SECTIONS = [
    { id: "overview", label: "Overview", icon: Gauge },
    { id: "tickets", label: "Tickets", icon: ClipboardList },
    { id: "roles", label: "Reaction Roles", icon: ListChecks },
    { id: "youtube", label: "YouTube", icon: Video },
    { id: "spreadsheets", label: "Spreadsheets", icon: FileSpreadsheet },
    { id: "members", label: "Members", icon: Users },
    { id: "logs", label: "Logs", icon: FileText },
    { id: "docs", label: "Documentation", icon: BookOpen },
    { id: "server", label: "Server", icon: Bot }
];

const DOCS_URL = "/docs/dca-bot-complete-guide.html";

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function makeId(prefix) {
    return `${prefix}-${Math.random().toString(16).slice(2, 10)}`;
}

async function fetchJson(url, options = {}) {
    const response = await fetch(url, {
        ...options,
        headers: {
            ...(options.body && !(options.body instanceof File) ? { "Content-Type": "application/json" } : {}),
            ...(options.headers || {})
        }
    });
    const data = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(data.error || data.message || `Request failed (${response.status})`);
    return data;
}

function authMessageFromQuery() {
    const params = new URLSearchParams(window.location.search);
    const auth = params.get("auth");
    const detail = params.get("detail");

    if (!auth) return "";

    const detailMessages = {
        oauth_state: "Discord sign in expired or lost its state cookie. Start from the dashboard login button again and make sure cookies are allowed.",
        missing_setup: "Dashboard OAuth setup is incomplete on the server.",
        oauth_exchange: "Discord rejected the OAuth exchange. Check DISCORD_CLIENT_SECRET and make sure DISCORD_REDIRECT_URI exactly matches the Discord Developer Portal redirect.",
        missing_access_token: "Discord did not return an access token.",
        guild_member_lookup: "Discord could not verify you in the configured server. Check DISCORD_GUILD_ID/community guild ID and make sure you are in that server.",
        discord_unauthorized: "Discord rejected one of the dashboard credentials. Check the bot token, client ID, and client secret.",
        discord_rate_limited: "Discord rate-limited the dashboard login. Wait a minute and try again.",
        discord_callback: "Discord sign in failed during the callback."
    };

    const authMessages = {
        denied: "Your Discord account is missing the required dashboard access role.",
        setup: "Dashboard OAuth setup is incomplete on the server.",
        error: detailMessages[detail] || "Discord sign in failed. Check the dashboard service logs on Render."
    };

    window.history.replaceState({}, document.title, window.location.pathname || "/dashboard");
    return authMessages[auth] || "";
}

function IconButton({ children, icon: Icon, variant = "primary", busy = false, ...props }) {
    return (
        <button className={`button ${variant}`} disabled={busy || props.disabled} {...props}>
            {busy ? <Loader2 className="spin" size={16} /> : Icon ? <Icon size={16} /> : null}
            <span>{children}</span>
        </button>
    );
}

function Field({ label, children, wide = false }) {
    return (
        <label className={`field ${wide ? "wide" : ""}`}>
            <span>{label}</span>
            {children}
        </label>
    );
}

function TextInput(props) {
    return <input className="input" {...props} />;
}

function TimeInput(props) {
    return (
        <div className="unit-input">
            <input className="input" type="number" {...props} />
            <span>min</span>
        </div>
    );
}

function TextArea(props) {
    return <textarea className="input textarea" {...props} />;
}

function Toggle({ label, checked, onChange }) {
    return (
        <button type="button" className={`toggle ${checked ? "on" : ""}`} onClick={() => onChange(!checked)}>
            <span className="toggle-switch" />
            <span>{label}</span>
        </button>
    );
}

function SelectField({ label, value, onChange, options, placeholder = "Not set" }) {
    return (
        <Field label={label}>
            <select className="input" value={value || ""} onChange={event => onChange(event.target.value)}>
                <option value="">{placeholder}</option>
                {options.map(option => (
                    <option key={option.id} value={option.id}>{option.name}</option>
                ))}
            </select>
        </Field>
    );
}

function combineLookupOptions(...groups) {
    const seen = new Set();
    const options = [];

    for (const group of groups) {
        for (const item of group.items || []) {
            if (!item?.id || seen.has(item.id)) continue;
            seen.add(item.id);
            options.push({
                ...item,
                name: `${group.label} / ${item.name}`
            });
        }
    }

    return options;
}

function normalizeSearch(value) {
    return String(value || "")
        .normalize("NFKD")
        .replace(/[\u0300-\u036f]/g, "")
        .toLowerCase()
        .replace(/\s+/g, " ")
        .trim();
}

function collectSearchValues(value, output = []) {
    if (value === null || value === undefined) return output;

    if (["string", "number", "boolean"].includes(typeof value)) {
        output.push(String(value));
        return output;
    }

    if (Array.isArray(value)) {
        value.forEach(item => collectSearchValues(item, output));
        return output;
    }

    if (typeof value === "object") {
        Object.entries(value).forEach(([key, item]) => {
            output.push(key);
            collectSearchValues(item, output);
        });
    }

    return output;
}

function makeTicketSearchEntry(ticket) {
    const name = normalizeSearch([
        ticket.threadName,
        ticket.applicantTag,
        ticket.applicantUsername,
        ticket.applicantId,
        ticket.threadId,
        ticket.team,
        ticket.outcome
    ].filter(Boolean).join(" "));
    const text = normalizeSearch(collectSearchValues(ticket).join(" "));

    return {
        ticket,
        name,
        text,
        haystack: `${name} ${text}`
    };
}

function filterTicketIndex(index, query) {
    const terms = normalizeSearch(query).split(" ").filter(Boolean);
    if (!terms.length) return index.map(entry => entry.ticket);

    return index
        .map(entry => {
            let score = 0;
            for (const term of terms) {
                if (!entry.haystack.includes(term)) return null;
                if (entry.name.startsWith(term)) score += 10;
                if (entry.name.includes(term)) score += 8;
                if (entry.text.includes(term)) score += 1;
            }

            return { ticket: entry.ticket, score };
        })
        .filter(Boolean)
        .sort((a, b) => {
            if (b.score !== a.score) return b.score - a.score;
            return String(b.ticket.updatedAt || b.ticket.createdAt || "").localeCompare(String(a.ticket.updatedAt || a.ticket.createdAt || ""));
        })
        .map(entry => entry.ticket);
}

function ticketSearchSnippet(ticket, query) {
    const terms = normalizeSearch(query).split(" ").filter(Boolean);
    if (!terms.length) return "";

    const source = collectSearchValues(ticket)
        .join(" ")
        .replace(/\s+/g, " ")
        .trim();
    const lower = source.toLowerCase();
    const term = terms.find(item => lower.includes(item));
    if (!term) return "";

    const index = lower.indexOf(term);
    const start = Math.max(0, index - 64);
    const end = Math.min(source.length, index + 150);
    const prefix = start > 0 ? "... " : "";
    const suffix = end < source.length ? " ..." : "";
    return `${prefix}${source.slice(start, end)}${suffix}`;
}

function SectionCard({ title, icon: Icon, children, actions = null }) {
    return (
        <section className="panel">
            <div className="panel-head">
                <div className="panel-title">
                    {Icon ? <Icon size={20} /> : null}
                    <h2>{title}</h2>
                </div>
                {actions ? <div className="panel-actions">{actions}</div> : null}
            </div>
            {children}
        </section>
    );
}

function StatCard({ label, value, icon: Icon, tone = "teal" }) {
    return (
        <div className={`stat ${tone}`}>
            <div>
                <span>{label}</span>
                <strong>{value}</strong>
            </div>
            {Icon ? <Icon size={22} /> : null}
        </div>
    );
}

export default function App() {
    const [section, setSection] = useState("overview");
    const [me, setMe] = useState(null);
    const [config, setConfig] = useState(null);
    const [lookups, setLookups] = useState({ channels: [], roles: [] });
    const [botLogs, setBotLogs] = useState([]);
    const [recruitmentLogs, setRecruitmentLogs] = useState([]);
    const [tickets, setTickets] = useState([]);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [notice, setNotice] = useState("");
    const [error, setError] = useState("");
    const [busyAction, setBusyAction] = useState("");
    const [selectedTranscript, setSelectedTranscript] = useState(null);
    const [transcriptLoading, setTranscriptLoading] = useState("");
    const [ticketSearch, setTicketSearch] = useState("");

    const channels = lookups.channels || [];
    const roles = lookups.roles || [];
    const communityChannels = lookups.community?.channels || channels;
    const communityRoles = lookups.community?.roles || roles;
    const recruitmentChannels = lookups.recruitment?.channels || channels;
    const recruitmentRoles = lookups.recruitment?.roles || roles;
    const communityServer = lookups.community?.guild || lookups.guild || null;
    const recruitmentServer = lookups.recruitment?.guild || lookups.guild || null;
    const communityServerLabel = communityServer?.name || config?.bot?.communityGuildId || config?.bot?.guildId || "Community server not set";
    const recruitmentServerLabel = recruitmentServer?.name || config?.bot?.recruitmentGuildId || config?.bot?.guildId || "Recruitment server not set";
    const serverChannels = useMemo(
        () => combineLookupOptions(
            { label: "Community", items: communityChannels },
            { label: "Recruitment", items: recruitmentChannels }
        ),
        [communityChannels, recruitmentChannels]
    );
    const serverRoles = useMemo(
        () => combineLookupOptions(
            { label: "Community", items: communityRoles },
            { label: "Recruitment", items: recruitmentRoles }
        ),
        [communityRoles, recruitmentRoles]
    );
    const spreadsheetChannels = serverChannels;
    const spreadsheetRoles = serverRoles;

    const activeTickets = useMemo(
        () => tickets.filter(ticket => ticket.status === "open").length,
        [tickets]
    );
    const ticketSearchIndex = useMemo(
        () => tickets.map(makeTicketSearchEntry),
        [tickets]
    );
    const filteredTickets = useMemo(
        () => filterTicketIndex(ticketSearchIndex, ticketSearch),
        [ticketSearchIndex, ticketSearch]
    );

    useEffect(() => {
        loadInitial();
    }, []);

    async function loadInitial() {
        const authMessage = authMessageFromQuery();
        setLoading(true);
        setError(authMessage);

        try {
            const profile = await fetchJson("/api/dashboard/me");
            setMe(profile);
            if (profile.authenticated) await loadDashboard();
        } catch (err) {
            setError(authMessage ? `${authMessage} ${err.message}` : err.message);
        } finally {
            setLoading(false);
        }
    }

    async function loadDashboard() {
        const data = await fetchJson("/api/dashboard/config");
        setConfig(data.config);
        setLookups(data.lookups || { channels: [], roles: [] });
        setBotLogs(data.botLogs || []);
        setRecruitmentLogs(data.recruitmentLogs || data.logs || []);
        setTickets(data.tickets || []);
    }

    function patch(path, value) {
        setConfig(current => {
            const next = clone(current);
            const keys = path.split(".");
            let cursor = next;
            for (const key of keys.slice(0, -1)) cursor = cursor[key];
            cursor[keys.at(-1)] = value;
            return next;
        });
    }

    function patchItem(path, index, patchValue) {
        setConfig(current => {
            const next = clone(current);
            const keys = path.split(".");
            let list = next;
            for (const key of keys) list = list[key];
            list[index] = { ...list[index], ...patchValue };
            return next;
        });
    }

    function pushItem(path, value) {
        setConfig(current => {
            const next = clone(current);
            const keys = path.split(".");
            let list = next;
            for (const key of keys) list = list[key];
            list.push(value);
            return next;
        });
    }

    function removeItem(path, index) {
        setConfig(current => {
            const next = clone(current);
            const keys = path.split(".");
            let list = next;
            for (const key of keys) list = list[key];
            list.splice(index, 1);
            return next;
        });
    }

    async function saveConfig() {
        setSaving(true);
        setError("");
        setNotice("");

        try {
            const data = await fetchJson("/api/dashboard/config", {
                method: "PUT",
                body: JSON.stringify(config)
            });
            setConfig(data.config);
            setNotice("Configuration saved.");
            await loadDashboard();
        } catch (err) {
            setError(err.message);
        } finally {
            setSaving(false);
        }
    }

    async function runAction(name, url) {
        setBusyAction(name);
        setError("");
        setNotice("");

        try {
            const data = await fetchJson(url, { method: "POST" });
            if (data.config) setConfig(data.config);
            setNotice(data.sync?.reason || data.reason || "Action completed.");
            await loadDashboard();
        } catch (err) {
            setError(err.message);
        } finally {
            setBusyAction("");
        }
    }

    async function uploadTutorial(tutorialId, file) {
        if (!file) return;
        setBusyAction(`upload-${tutorialId}`);
        setError("");

        try {
            const data = await fetch(`/api/dashboard/recruitment/tutorials/${encodeURIComponent(tutorialId)}/upload`, {
                method: "POST",
                headers: {
                    "Content-Type": file.type || "application/octet-stream",
                    "x-file-name": file.name
                },
                body: file
            }).then(async response => {
                const body = await response.json().catch(() => ({}));
                if (!response.ok) throw new Error(body.error || "Upload failed.");
                return body;
            });

            setConfig(data.config);
            setNotice(`Uploaded ${data.tutorial.label}.`);
        } catch (err) {
            setError(err.message);
        } finally {
            setBusyAction("");
        }
    }

    async function openTranscript(ticket) {
        setTranscriptLoading(ticket.threadId);
        setError("");

        try {
            const data = await fetchJson(`/api/dashboard/tickets/${encodeURIComponent(ticket.threadId)}/transcript`);
            setTickets(current => current.map(item =>
                item.threadId === data.threadId
                    ? {
                        ...item,
                        transcript: data.transcript || item.transcript || null,
                        transcriptPreview: data.transcriptPreview || item.transcriptPreview || "",
                        applicantThreadImages: data.applicantThreadImages || item.applicantThreadImages || []
                    }
                    : item
            ));
            setSelectedTranscript(data);
        } catch (err) {
            setError(err.message);
        } finally {
            setTranscriptLoading("");
        }
    }

    if (loading) {
        return (
            <div className="app-shell center-shell">
                <Loader2 className="spin" size={34} />
            </div>
        );
    }

    if (!me?.authenticated) {
        return (
            <div className="login-screen">
                <div className="login-card">
                    <div className="brand-mark"><Lock size={28} /></div>
                    <h1>DCA Bot Dashboard</h1>
                    {me?.setup?.configured ? (
                        <a className="button primary login-button" href="/auth/discord">
                            <Bot size={16} />
                            <span>Sign in with Discord</span>
                        </a>
                    ) : (
                        <div className="missing-env">
                            <strong>Missing setup</strong>
                            <p>{(me?.setup?.missing || []).join(", ") || "Dashboard OAuth is not configured."}</p>
                        </div>
                    )}
                    {error ? <p className="error-line">{error}</p> : null}
                </div>
            </div>
        );
    }

    if (!config) return null;

    return (
        <div className="app-shell">
            <aside className="sidebar">
                <div className="brand">
                    <div className="brand-mark"><Bot size={24} /></div>
                    <div>
                        <strong>DCA Control</strong>
                        <span>{lookups.guild?.name || "Discord server"}</span>
                    </div>
                </div>

                <nav>
                    {SECTIONS.map(item => {
                        const Icon = item.icon;
                        return (
                            <button
                                key={item.id}
                                className={section === item.id ? "active" : ""}
                                onClick={() => setSection(item.id)}
                            >
                                <Icon size={18} />
                                <span>{item.label}</span>
                                <ChevronRight size={15} />
                            </button>
                        );
                    })}
                </nav>

                <div className="user-pill">
                    {me.user?.avatarUrl ? <img src={me.user.avatarUrl} alt="" /> : <div className="avatar-fallback" />}
                    <div>
                        <strong>{me.user?.globalName || me.user?.username}</strong>
                        <a href="/auth/logout">Sign out</a>
                    </div>
                </div>
            </aside>

            <main>
                <header className="topbar">
                    <div>
                        <span className="eyebrow">Recruitment Operations</span>
                        <h1>{SECTIONS.find(item => item.id === section)?.label}</h1>
                        <div className="server-context">
                            <span><strong>Recruitment server</strong>{recruitmentServerLabel}</span>
                            <span><strong>Community server</strong>{communityServerLabel}</span>
                        </div>
                    </div>
                    <div className="topbar-actions">
                        <IconButton icon={RefreshCw} variant="ghost" onClick={loadDashboard}>Refresh</IconButton>
                        <IconButton icon={Save} busy={saving} onClick={saveConfig}>Save</IconButton>
                    </div>
                </header>

                {notice ? <div className="notice success"><CheckCircle2 size={16} />{notice}</div> : null}
                {error ? <div className="notice danger">{error}</div> : null}

                {section === "overview" ? renderOverview() : null}
                {section === "tickets" ? renderTickets() : null}
                {section === "roles" ? renderReactionRoles() : null}
                {section === "youtube" ? renderYoutube() : null}
                {section === "spreadsheets" ? renderSpreadsheets() : null}
                {section === "members" ? renderMembers() : null}
                {section === "logs" ? renderLogs() : null}
                {section === "docs" ? renderDocs() : null}
                {section === "server" ? renderServer() : null}
            </main>
            {selectedTranscript ? renderTranscriptModal() : null}
        </div>
    );

    function renderOverview() {
        return (
            <div className="content-grid">
                <StatCard label="Open tickets" value={activeTickets} icon={ClipboardList} />
                <StatCard label="Reaction panels" value={config.reactionRoles.length} icon={ListChecks} tone="amber" />
                <StatCard label="YouTube feeds" value={config.youtube.feeds.length} icon={Radio} tone="blue" />
                <StatCard label="Teams tracked" value={config.memberCounts.teams.length} icon={Users} tone="rose" />

                <SectionCard title="Quick Actions" icon={Gauge} actions={
                    <>
                        <IconButton
                            icon={RefreshCw}
                            variant="secondary"
                            busy={busyAction === "panel"}
                            onClick={() => runAction("panel", "/api/dashboard/recruitment/panel/sync")}
                        >
                            Sync Apply Panel
                        </IconButton>
                        <IconButton
                            icon={RefreshCw}
                            variant="secondary"
                            busy={busyAction === "roles"}
                            onClick={() => runAction("roles", "/api/dashboard/reaction-roles/sync")}
                        >
                            Sync Reaction Roles
                        </IconButton>
                        <IconButton
                            icon={RefreshCw}
                            variant="secondary"
                            busy={busyAction === "members"}
                            onClick={() => runAction("members", "/api/dashboard/member-counts/sync")}
                        >
                            Sync Member Count
                        </IconButton>
                    </>
                }>
                    <div className="status-grid">
                        <div><span>Bot</span><strong>{me.bot?.ready ? me.bot.user?.tag : "Not reachable"}</strong></div>
                        <div><span>Recruitment server</span><strong>{recruitmentServerLabel}</strong></div>
                        <div><span>Community server</span><strong>{communityServerLabel}</strong></div>
                        <div><span>Ticket panel</span><strong>{config.recruitment.panelMessageId || "Not posted"}</strong></div>
                        <div><span>Member count</span><strong>{config.memberCounts.messageId || "Not posted"}</strong></div>
                    </div>
                </SectionCard>

                <SectionCard title="Recent Ticket Outcomes" icon={MessageSquare}>
                    <LogList items={recruitmentLogs.slice(0, 6)} type="recruitment" />
                </SectionCard>
            </div>
        );
    }

    function renderDocs() {
        return (
            <div className="docs-page">
                <section className="docs-hero">
                    <div>
                        <span className="eyebrow">Operator handbook</span>
                        <h2>DCA Bot Suite Documentation</h2>
                        <p>Complete staff guide for dashboard configuration, recruitment tickets, team-event spreadsheets, member counts, reaction roles, feeds, moderation, event helpers, logs, and troubleshooting. The guide now loads as a native webpage inside the dashboard.</p>
                    </div>
                    <a className="button secondary" href={DOCS_URL} target="_blank" rel="noreferrer">
                        <BookOpen size={16} />
                        <span>Open Web Guide</span>
                        <ExternalLink size={14} />
                    </a>
                </section>

                <section className="docs-web-shell panel" aria-label="DCA Bot Suite complete documentation">
                    <iframe
                        className="docs-web-frame"
                        src={DOCS_URL}
                        title="DCA Bot Suite complete documentation"
                    />
                </section>
            </div>
        );
    }

    function renderTickets() {
        return (
            <div className="stack">
                <SectionCard title="Ticket Configuration" icon={ClipboardList} actions={
                    <>
                        <IconButton
                            icon={RefreshCw}
                            variant="secondary"
                            busy={busyAction === "panel"}
                            onClick={() => runAction("panel", "/api/dashboard/recruitment/panel/sync")}
                        >
                            Sync Panel
                        </IconButton>
                        <IconButton
                            icon={RefreshCw}
                            variant="secondary"
                            busy={busyAction === "ban-list"}
                            onClick={() => runAction("ban-list", "/api/dashboard/recruitment/ban-list/sync")}
                        >
                            Sync Ban List
                        </IconButton>
                    </>
                }>
                    <div className="form-grid">
                        <Toggle label="Recruitment enabled" checked={config.recruitment.enabled} onChange={value => patch("recruitment.enabled", value)} />
                        <Toggle label="Private threads" checked={config.recruitment.privateThreads} onChange={value => patch("recruitment.privateThreads", value)} />
                        <Toggle label="Transcript on close" checked={config.recruitment.transcriptOnClose} onChange={value => patch("recruitment.transcriptOnClose", value)} />
                        <div className="fixed-setting"><span>Close behavior</span><strong>Lock + archive</strong></div>
                        <SelectField label="Panel channel" value={config.recruitment.panelChannelId} onChange={value => patch("recruitment.panelChannelId", value)} options={serverChannels} />
                        <SelectField label="Image log channel" value={config.recruitment.logChannelId} onChange={value => patch("recruitment.logChannelId", value)} options={serverChannels} />
                        <SelectField label="Ban list channel" value={config.recruitment.banListChannelId || ""} onChange={value => patch("recruitment.banListChannelId", value)} options={serverChannels} />
                        <SelectField label="Tutorial upload channel" value={config.recruitment.tutorialUploadChannelId} onChange={value => patch("recruitment.tutorialUploadChannelId", value)} options={serverChannels} />
                        <SelectField label="Recruiter role" value={config.recruitment.recruiterRoleId} onChange={value => patch("recruitment.recruiterRoleId", value)} options={serverRoles} />
                        <Field label="Screenshot DM user ID"><TextInput value={config.recruitment.screenshotDmUserId || ""} onChange={event => patch("recruitment.screenshotDmUserId", event.target.value)} /></Field>
                        <Field label="Max open tickets"><TextInput type="number" min="1" max="10" value={config.recruitment.maxOpenTicketsPerUser} onChange={event => patch("recruitment.maxOpenTicketsPerUser", Number(event.target.value))} /></Field>
                        <Field label="Panel title"><TextInput value={config.recruitment.panelTitle} onChange={event => patch("recruitment.panelTitle", event.target.value)} /></Field>
                        <Field label="Panel color"><TextInput type="color" value={config.recruitment.panelColor} onChange={event => patch("recruitment.panelColor", event.target.value)} /></Field>
                        <Field label="Panel description" wide><TextArea rows={4} value={config.recruitment.panelDescription} onChange={event => patch("recruitment.panelDescription", event.target.value)} /></Field>
                        <Field label="Question intro" wide><TextArea rows={3} value={config.recruitment.questionsIntro} onChange={event => patch("recruitment.questionsIntro", event.target.value)} /></Field>
                        <Field label="Questions" wide><TextArea rows={7} value={config.recruitment.questions} onChange={event => patch("recruitment.questions", event.target.value)} /></Field>
                    </div>
                </SectionCard>

                <SectionCard title="Destination Invite" icon={Send}>
                    <div className="form-grid">
                        <Field label="Destination server ID"><TextInput value={config.recruitment.inviteGuildId || ""} onChange={event => patch("recruitment.inviteGuildId", event.target.value)} /></Field>
                        <SelectField label="Invite channel" value={config.recruitment.inviteChannelId || ""} onChange={value => patch("recruitment.inviteChannelId", value)} options={serverChannels} />
                        <Field label="Invite message" wide><TextArea rows={3} value={config.recruitment.inviteMessage || ""} onChange={event => patch("recruitment.inviteMessage", event.target.value)} /></Field>
                    </div>
                </SectionCard>

                <SectionCard title="Closing Outcomes" icon={CheckCircle2}>
                    <div className="chips-edit">
                        {config.recruitment.teams.map((team, index) => (
                            <label className="chip-input" key={`${team}-${index}`}>
                                <input value={team} onChange={event => {
                                    const next = [...config.recruitment.teams];
                                    next[index] = event.target.value;
                                    patch("recruitment.teams", next);
                                }} />
                                <button type="button" onClick={() => {
                                    const next = [...config.recruitment.teams];
                                    next.splice(index, 1);
                                    patch("recruitment.teams", next);
                                }}><Trash2 size={14} /></button>
                            </label>
                        ))}
                        <IconButton icon={Plus} variant="ghost" onClick={() => patch("recruitment.teams", [...config.recruitment.teams, "New Team"])}>Add Team</IconButton>
                    </div>
                </SectionCard>

                <SectionCard title="Tutorial Videos" icon={Upload} actions={
                    <IconButton icon={Plus} variant="ghost" onClick={() => pushItem("recruitment.tutorials", {
                        id: makeId("tutorial"),
                        label: "New Tutorial",
                        description: "",
                        videoUrl: "",
                        enabled: true
                    })}>Add Tutorial</IconButton>
                }>
                    <div className="cards-grid">
                        {config.recruitment.tutorials.map((tutorial, index) => (
                            <div className="edit-card" key={tutorial.id}>
                                <div className="card-row">
                                    <Toggle label="Enabled" checked={tutorial.enabled} onChange={value => patchItem("recruitment.tutorials", index, { enabled: value })} />
                                    <button className="icon-only" onClick={() => removeItem("recruitment.tutorials", index)}><Trash2 size={16} /></button>
                                </div>
                                <Field label="ID"><TextInput value={tutorial.id} onChange={event => patchItem("recruitment.tutorials", index, { id: event.target.value })} /></Field>
                                <Field label="Label"><TextInput value={tutorial.label} onChange={event => patchItem("recruitment.tutorials", index, { label: event.target.value })} /></Field>
                                <Field label="Description"><TextArea rows={3} value={tutorial.description} onChange={event => patchItem("recruitment.tutorials", index, { description: event.target.value })} /></Field>
                                <Field label="Video URL"><TextInput value={tutorial.videoUrl} onChange={event => patchItem("recruitment.tutorials", index, { videoUrl: event.target.value })} /></Field>
                                <label className="upload-zone">
                                    {busyAction === `upload-${tutorial.id}` ? <Loader2 className="spin" size={18} /> : <Upload size={18} />}
                                    <span>Upload video</span>
                                    <input type="file" accept="video/mp4,video/webm,video/quicktime" onChange={event => uploadTutorial(tutorial.id, event.target.files?.[0])} />
                                </label>
                            </div>
                        ))}
                    </div>
                </SectionCard>
            </div>
        );
    }

    function renderReactionRoles() {
        return (
            <SectionCard title="Reaction Role Panels" icon={ListChecks} actions={
                <>
                    <IconButton icon={Plus} variant="ghost" onClick={() => pushItem("reactionRoles", {
                        id: makeId("reaction"),
                        name: "New Panel",
                        enabled: true,
                        channelId: "",
                        messageId: "",
                        message: "React below to choose a role.",
                        options: []
                    })}>Add Panel</IconButton>
                    <IconButton icon={RefreshCw} variant="secondary" busy={busyAction === "roles"} onClick={() => runAction("roles", "/api/dashboard/reaction-roles/sync")}>Sync</IconButton>
                </>
            }>
                <div className="cards-grid">
                    {config.reactionRoles.map((group, groupIndex) => (
                        <div className="edit-card large" key={group.id}>
                            <div className="card-row">
                                <Toggle label="Enabled" checked={group.enabled} onChange={value => patchItem("reactionRoles", groupIndex, { enabled: value })} />
                                <button className="icon-only" onClick={() => removeItem("reactionRoles", groupIndex)}><Trash2 size={16} /></button>
                            </div>
                            <Field label="Panel name"><TextInput value={group.name} onChange={event => patchItem("reactionRoles", groupIndex, { name: event.target.value })} /></Field>
                            <SelectField label="Channel" value={group.channelId} onChange={value => patchItem("reactionRoles", groupIndex, { channelId: value })} options={serverChannels} />
                            <Field label="Message ID"><TextInput value={group.messageId} onChange={event => patchItem("reactionRoles", groupIndex, { messageId: event.target.value })} /></Field>
                            <Field label="Message"><TextArea rows={5} value={group.message} onChange={event => patchItem("reactionRoles", groupIndex, { message: event.target.value })} /></Field>
                            <div className="option-list">
                                {group.options.map((option, optionIndex) => (
                                    <div className="option-row" key={`${option.emoji}-${option.roleId}-${optionIndex}`}>
                                        <TextInput value={option.emoji} onChange={event => updateReactionOption(groupIndex, optionIndex, { emoji: event.target.value })} />
                                        <select className="input" value={option.roleId} onChange={event => updateReactionOption(groupIndex, optionIndex, { roleId: event.target.value })}>
                                            <option value="">Role</option>
                                            {serverRoles.map(role => <option key={role.id} value={role.id}>{role.name}</option>)}
                                        </select>
                                        <TextInput value={option.label || ""} placeholder="Label" onChange={event => updateReactionOption(groupIndex, optionIndex, { label: event.target.value })} />
                                        <button className="icon-only" onClick={() => removeReactionOption(groupIndex, optionIndex)}><Trash2 size={16} /></button>
                                    </div>
                                ))}
                            </div>
                            <IconButton icon={Plus} variant="ghost" onClick={() => addReactionOption(groupIndex)}>Add Option</IconButton>
                        </div>
                    ))}
                </div>
            </SectionCard>
        );
    }

    function renderYoutube() {
        return (
            <SectionCard title="YouTube Feed" icon={Video} actions={
                <IconButton icon={Plus} variant="ghost" onClick={() => pushItem("youtube.feeds", {
                    id: "",
                    name: "New Channel",
                    channelId: "",
                    enabled: true,
                    lastVideoId: ""
                })}>Add Feed</IconButton>
            }>
                <div className="form-grid compact">
                    <Toggle label="Enabled" checked={config.youtube.enabled} onChange={value => patch("youtube.enabled", value)} />
                    <SelectField label="Default post channel" value={config.youtube.defaultChannelId} onChange={value => patch("youtube.defaultChannelId", value)} options={serverChannels} />
                    <Field label="Check interval"><TextInput type="number" min="1" max="1440" value={config.youtube.checkIntervalMinutes} onChange={event => patch("youtube.checkIntervalMinutes", Number(event.target.value))} /></Field>
                    <Field label="Announcement" wide><TextArea rows={3} value={config.youtube.announcementTemplate} onChange={event => patch("youtube.announcementTemplate", event.target.value)} /></Field>
                </div>
                <div className="cards-grid">
                    {config.youtube.feeds.map((feed, index) => (
                        <div className="edit-card" key={`${feed.id}-${index}`}>
                            <div className="card-row">
                                <Toggle label="Enabled" checked={feed.enabled} onChange={value => patchItem("youtube.feeds", index, { enabled: value })} />
                                <button className="icon-only" onClick={() => removeItem("youtube.feeds", index)}><Trash2 size={16} /></button>
                            </div>
                            <Field label="Feed name"><TextInput value={feed.name} onChange={event => patchItem("youtube.feeds", index, { name: event.target.value })} /></Field>
                            <Field label="YouTube channel ID"><TextInput value={feed.id} onChange={event => patchItem("youtube.feeds", index, { id: event.target.value })} /></Field>
                            <SelectField label="Post channel" value={feed.channelId} onChange={value => patchItem("youtube.feeds", index, { channelId: value })} options={serverChannels} placeholder="Default channel" />
                            <Field label="Last video ID"><TextInput value={feed.lastVideoId || ""} onChange={event => patchItem("youtube.feeds", index, { lastVideoId: event.target.value })} /></Field>
                        </div>
                    ))}
                </div>
            </SectionCard>
        );
    }

    function renderSpreadsheets() {
        const spreadsheets = config.spreadsheets || {
            enabled: false,
            sessionWindowMinutes: 1,
            outputFormat: "xlsx",
            libreOfficePath: "",
            geminiModel: "gemini-2.5-flash",
            geminiTimeoutMs: 300000,
            geminiMaxRetries: 4,
            rawDataRetentionDays: 31,
            imageRetentionDays: 7,
            teams: []
        };

        return (
            <div className="stack">
                <SectionCard title="Spreadsheet Gemini Settings" icon={FileSpreadsheet}>
                    <div className="form-grid compact">
                        <Toggle label="Enabled" checked={Boolean(spreadsheets.enabled)} onChange={value => patch("spreadsheets.enabled", value)} />
                        <Field label="Grouping window">
                            <TimeInput min="1" max="30" value={spreadsheets.sessionWindowMinutes || 1} onChange={event => patch("spreadsheets.sessionWindowMinutes", Number(event.target.value))} />
                        </Field>
                        <Field label="Output format">
                            <select className="input" value={spreadsheets.outputFormat || "xlsx"} onChange={event => patch("spreadsheets.outputFormat", event.target.value)}>
                                <option value="xlsx">XLSX</option>
                                <option value="fods">Flat ODS</option>
                            </select>
                        </Field>
                        <Field label="LibreOffice path"><TextInput placeholder="Leave blank; Node writes XLSX" value={spreadsheets.libreOfficePath || ""} onChange={event => patch("spreadsheets.libreOfficePath", event.target.value)} /></Field>
                        <Field label="Gemini Flash model"><TextInput value={spreadsheets.geminiModel || "gemini-2.5-flash"} onChange={event => patch("spreadsheets.geminiModel", event.target.value)} /></Field>
                        <Field label="Gemini timeout ms"><TextInput type="number" min="30000" max="900000" value={spreadsheets.geminiTimeoutMs || 300000} onChange={event => patch("spreadsheets.geminiTimeoutMs", Number(event.target.value))} /></Field>
                        <Field label="Gemini retries"><TextInput type="number" min="0" max="10" value={spreadsheets.geminiMaxRetries ?? 4} onChange={event => patch("spreadsheets.geminiMaxRetries", Number(event.target.value))} /></Field>
                        <Field label="Raw data retention days"><TimeInput min="1" max="370" value={spreadsheets.rawDataRetentionDays || 31} onChange={event => patch("spreadsheets.rawDataRetentionDays", Number(event.target.value))} /></Field>
                        <Field label="Local image retention days"><TimeInput min="1" max="90" value={spreadsheets.imageRetentionDays || 7} onChange={event => patch("spreadsheets.imageRetentionDays", Number(event.target.value))} /></Field>
                    </div>
                </SectionCard>

                <SectionCard title="Team Spreadsheet Channels" icon={FileSpreadsheet} actions={
                    <IconButton icon={Plus} variant="ghost" onClick={() => pushItem("spreadsheets.teams", {
                        id: makeId("spreadsheet-team"),
                        name: "New Team",
                        enabled: false,
                        monitoredChannelId: "",
                        outputChannelId: "",
                        accessRoleId: "",
                        ownTeamAliases: [],
                        ownPlayerAliases: [],
                        autoProcess: true
                    })}>Add Team</IconButton>
                }>
                    <div className="cards-grid">
                        {(spreadsheets.teams || []).map((team, index) => (
                            <div className="edit-card" key={team.id || index}>
                                <div className="card-row">
                                    <Toggle label="Enabled" checked={Boolean(team.enabled)} onChange={value => patchItem("spreadsheets.teams", index, { enabled: value })} />
                                    <button className="icon-only" onClick={() => removeItem("spreadsheets.teams", index)}><Trash2 size={16} /></button>
                                </div>
                                <Field label="Team name"><TextInput value={team.name || ""} onChange={event => patchItem("spreadsheets.teams", index, { name: event.target.value })} /></Field>
                                <Field label="Data team ID"><TextInput value={team.id || ""} onChange={event => patchItem("spreadsheets.teams", index, { id: event.target.value })} /></Field>
                                <SelectField label="Monitored channel" value={team.monitoredChannelId || ""} onChange={value => patchItem("spreadsheets.teams", index, { monitoredChannelId: value })} options={spreadsheetChannels} />
                                <SelectField label="Output channel" value={team.outputChannelId || ""} onChange={value => patchItem("spreadsheets.teams", index, { outputChannelId: value })} options={spreadsheetChannels} placeholder="Submission channel" />
                                <SelectField label="Team access role" value={team.accessRoleId || ""} onChange={value => patchItem("spreadsheets.teams", index, { accessRoleId: value })} options={spreadsheetRoles} placeholder="Admins only" />
                                <Field label="Own team aliases"><TextInput value={(team.ownTeamAliases || []).join(", ")} onChange={event => patchItem("spreadsheets.teams", index, { ownTeamAliases: event.target.value.split(",").map(item => item.trim()).filter(Boolean) })} /></Field>
                                <Field label="Known own players"><TextInput value={(team.ownPlayerAliases || []).join(", ")} onChange={event => patchItem("spreadsheets.teams", index, { ownPlayerAliases: event.target.value.split(",").map(item => item.trim()).filter(Boolean) })} /></Field>
                                <Toggle label="Auto process" checked={team.autoProcess !== false} onChange={value => patchItem("spreadsheets.teams", index, { autoProcess: value })} />
                            </div>
                        ))}
                    </div>
                </SectionCard>
            </div>
        );
    }

    function renderMembers() {
        return (
            <SectionCard title="Member Count And Auto-Assign Roles" icon={Users} actions={
                <>
                    <IconButton icon={Plus} variant="ghost" onClick={() => pushItem("memberCounts.teams", {
                        id: makeId("team"),
                        name: "New Team",
                        division: "",
                        players: 0,
                        recruitmentStatus: "Open",
                        recruitmentRoleId: "",
                        recruitmentRoleAutoAssignEnabled: false,
                        recruitmentRoleDelayMinutes: 0,
                        communityRoleId: "",
                        communityRoleAutoAssignEnabled: false,
                        communityRoleDelayMinutes: 0,
                        aliases: []
                    })}>Add Team</IconButton>
                    <IconButton icon={RefreshCw} variant="secondary" busy={busyAction === "members"} onClick={() => runAction("members", "/api/dashboard/member-counts/sync")}>Sync</IconButton>
                </>
            }>
                <div className="form-grid compact">
                    <Toggle label="Enabled" checked={config.memberCounts.enabled} onChange={value => patch("memberCounts.enabled", value)} />
                    <Toggle label="Update after ticket close" checked={config.memberCounts.updateOnRecruitmentClose} onChange={value => patch("memberCounts.updateOnRecruitmentClose", value)} />
                    <SelectField label="Count channel" value={config.memberCounts.channelId} onChange={value => patch("memberCounts.channelId", value)} options={serverChannels} />
                    <SelectField label="Rules role" value={config.recruitment.communityRulesRoleId || ""} onChange={value => patch("recruitment.communityRulesRoleId", value)} options={serverRoles} />
                    <Field label="Message ID"><TextInput value={config.memberCounts.messageId} onChange={event => patch("memberCounts.messageId", event.target.value)} /></Field>
                    <Field label="Title"><TextInput value={config.memberCounts.title} onChange={event => patch("memberCounts.title", event.target.value)} /></Field>
                </div>
                <div className="table-wrap member-table-wrap">
                    <table>
                        <thead><tr><th>Team</th><th>Division</th><th>Players</th><th>Status</th><th>Recruitment role</th><th>Recruitment auto</th><th>Recruitment delay (min)</th><th>Community role</th><th>After rules</th><th>Community delay (min)</th><th>Aliases</th><th></th></tr></thead>
                        <tbody>
                            {config.memberCounts.teams.map((team, index) => (
                                <tr key={team.id}>
                                    <td><TextInput value={team.name} onChange={event => patchItem("memberCounts.teams", index, { name: event.target.value })} /></td>
                                    <td><TextInput value={team.division || ""} onChange={event => patchItem("memberCounts.teams", index, { division: event.target.value })} /></td>
                                    <td><TextInput type="number" value={team.players} onChange={event => patchItem("memberCounts.teams", index, { players: Number(event.target.value) })} /></td>
                                    <td><TextInput value={team.recruitmentStatus} onChange={event => patchItem("memberCounts.teams", index, { recruitmentStatus: event.target.value })} /></td>
                                    <td>
                                        <select className="input" value={team.recruitmentRoleId || ""} onChange={event => patchItem("memberCounts.teams", index, { recruitmentRoleId: event.target.value })}>
                                            <option value="">No role</option>
                                            {serverRoles.map(role => <option key={role.id} value={role.id}>{role.name}</option>)}
                                        </select>
                                    </td>
                                    <td><Toggle label="Auto" checked={Boolean(team.recruitmentRoleAutoAssignEnabled)} onChange={value => patchItem("memberCounts.teams", index, { recruitmentRoleAutoAssignEnabled: value })} /></td>
                                    <td><TimeInput min="0" max="43200" value={team.recruitmentRoleDelayMinutes || 0} onChange={event => patchItem("memberCounts.teams", index, { recruitmentRoleDelayMinutes: Number(event.target.value) })} /></td>
                                    <td>
                                        <select className="input" value={team.communityRoleId || team.roleId || ""} onChange={event => patchItem("memberCounts.teams", index, { communityRoleId: event.target.value, roleId: event.target.value })}>
                                            <option value="">No role</option>
                                            {serverRoles.map(role => <option key={role.id} value={role.id}>{role.name}</option>)}
                                        </select>
                                    </td>
                                    <td><Toggle label="Rules" checked={Boolean(team.communityRoleAutoAssignEnabled || team.autoAssignEnabled)} onChange={value => patchItem("memberCounts.teams", index, { communityRoleAutoAssignEnabled: value, autoAssignEnabled: value })} /></td>
                                    <td><TimeInput min="0" max="43200" value={team.communityRoleDelayMinutes || team.autoAssignDelayMinutes || 0} onChange={event => patchItem("memberCounts.teams", index, { communityRoleDelayMinutes: Number(event.target.value), autoAssignDelayMinutes: Number(event.target.value) })} /></td>
                                    <td><TextInput value={(team.aliases || []).join(", ")} onChange={event => patchItem("memberCounts.teams", index, { aliases: event.target.value.split(",").map(item => item.trim()).filter(Boolean) })} /></td>
                                    <td><button className="icon-only" onClick={() => removeItem("memberCounts.teams", index)}><Trash2 size={16} /></button></td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                </div>
            </SectionCard>
        );
    }

    function renderLogs() {
        return (
            <div className="stack">
                <SectionCard title="Combined Bot Logs" icon={FileText}>
                    <LogList items={botLogs} type="bot" />
                </SectionCard>
                <SectionCard title="Recruitment Logs" icon={ClipboardList}>
                    <LogList items={recruitmentLogs} type="recruitment" />
                </SectionCard>
                <SectionCard title="Tickets" icon={MessageSquare}>
                    <div className="ticket-search-tools">
                        <label className="search-field">
                            <span>Indexed ticket search</span>
                            <div className="search-input">
                                <Search size={16} />
                                <input
                                    className="input"
                                    type="search"
                                    value={ticketSearch}
                                    placeholder="Search applicant, thread, outcome, OCR text, or transcript contents"
                                    onChange={event => setTicketSearch(event.target.value)}
                                />
                                {ticketSearch ? (
                                    <button type="button" className="search-clear" onClick={() => setTicketSearch("")} aria-label="Clear ticket search">
                                        <X size={15} />
                                    </button>
                                ) : null}
                            </div>
                        </label>
                        <div className="search-summary">
                            Showing {filteredTickets.length} of {tickets.length} tickets
                        </div>
                    </div>
                    <div className="table-wrap ticket-table-wrap">
                        {filteredTickets.length ? (
                            <table>
                                <thead><tr><th>Applicant</th><th>Status</th><th>Claimed</th><th>Outcome</th><th>Images</th><th>Thread</th><th>Updated</th><th></th></tr></thead>
                                <tbody>
                                    {filteredTickets.map(ticket => (
                                        <tr key={ticket.threadId}>
                                            <td className="ticket-name-cell">
                                                <strong>{ticket.applicantTag || ticket.applicantId}</strong>
                                                {ticket.applicantUsername || ticket.threadName ? <span>{ticket.applicantUsername || ticket.threadName}</span> : null}
                                                {ticketSearch ? <small>{ticketSearchSnippet(ticket, ticketSearch)}</small> : null}
                                            </td>
                                            <td><span className={`pill ${ticket.status}`}>{ticket.status}</span></td>
                                            <td>{ticket.claimedByTag || "-"}</td>
                                            <td>{ticket.team || ticket.outcome || "-"}</td>
                                            <td><span className="image-count"><Image size={14} />{(ticket.applicantThreadImages || []).length}</span></td>
                                            <td>{ticket.threadId}</td>
                                            <td>{formatDate(ticket.updatedAt || ticket.createdAt)}</td>
                                            <td>
                                                <button className="icon-only" title="View transcript" onClick={() => openTranscript(ticket)} disabled={transcriptLoading === ticket.threadId}>
                                                    {transcriptLoading === ticket.threadId ? <Loader2 className="spin" size={16} /> : <Eye size={16} />}
                                                </button>
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        ) : (
                            <div className="empty-state">No tickets match this search.</div>
                        )}
                    </div>
                </SectionCard>
            </div>
        );
    }

    function renderTranscriptModal() {
        const transcriptText = selectedTranscript.transcript?.text || selectedTranscript.transcriptPreview || "";
        const images = selectedTranscript.applicantThreadImages || [];

        return (
            <div className="modal-backdrop" role="presentation" onClick={() => setSelectedTranscript(null)}>
                <section className="transcript-modal" role="dialog" aria-modal="true" onClick={event => event.stopPropagation()}>
                    <div className="modal-head">
                        <div>
                            <span className="eyebrow">{selectedTranscript.status} ticket</span>
                            <h2>{selectedTranscript.applicantTag || selectedTranscript.threadId}</h2>
                        </div>
                        <button className="icon-only" onClick={() => setSelectedTranscript(null)}><X size={16} /></button>
                    </div>
                    <div className="modal-meta">
                        <span>{selectedTranscript.outcome || "No outcome"}</span>
                        <span>{formatDate(selectedTranscript.closedAt)}</span>
                        <span>{selectedTranscript.threadId}</span>
                    </div>
                    {images.length ? (
                        <div className="image-strip">
                            {images.slice(0, 8).map(image => (
                                <a key={image.url} href={image.url} target="_blank" rel="noreferrer">
                                    <img src={image.url} alt="" />
                                </a>
                            ))}
                        </div>
                    ) : null}
                    <pre className="transcript-box">{transcriptText || "No transcript was saved for this ticket."}</pre>
                </section>
            </div>
        );
    }

    function renderServer() {
        return (
            <div className="stack">
                <SectionCard title="Server Configuration" icon={Bot}>
                    <div className="form-grid">
                        <Field label="Fallback guild ID"><TextInput value={config.bot.guildId} onChange={event => patch("bot.guildId", event.target.value)} /></Field>
                        <Field label="Community server ID"><TextInput value={config.bot.communityGuildId || ""} onChange={event => patch("bot.communityGuildId", event.target.value)} /></Field>
                        <Field label="Recruitment server ID"><TextInput value={config.bot.recruitmentGuildId || ""} onChange={event => patch("bot.recruitmentGuildId", event.target.value)} /></Field>
                        <SelectField label="Dashboard access role" value={config.bot.dashboardAllowedRoleId} onChange={value => patch("bot.dashboardAllowedRoleId", value)} options={serverRoles} />
                        <SelectField label="Recruiter role" value={config.bot.recruiterRoleId} onChange={value => patch("bot.recruiterRoleId", value)} options={serverRoles} />
                        <SelectField label="Manager role" value={config.bot.managerRoleId} onChange={value => patch("bot.managerRoleId", value)} options={serverRoles} />
                        <SelectField label="Command log channel" value={config.bot.commandLogChannelId} onChange={value => patch("bot.commandLogChannelId", value)} options={serverChannels} />
                        <Field label="Dashboard URL"><TextInput value={config.bot.dashboardUrl || ""} onChange={event => patch("bot.dashboardUrl", event.target.value)} /></Field>
                        <Field label="Locale"><TextInput value={config.bot.locale} onChange={event => patch("bot.locale", event.target.value)} /></Field>
                    </div>
                </SectionCard>

                <SectionCard title="Logging" icon={Bell}>
                    <div className="form-grid compact">
                        <Toggle label="Logging enabled" checked={config.logging.enabled} onChange={value => patch("logging.enabled", value)} />
                        <SelectField label="Combined log channel" value={config.logging.channelId} onChange={value => patch("logging.channelId", value)} options={serverChannels} />
                        {Object.entries(config.logging.events).map(([key, value]) => (
                            <Toggle key={key} label={key} checked={value} onChange={next => patch(`logging.events.${key}`, next)} />
                        ))}
                    </div>
                </SectionCard>

                <SectionCard title="Welcome And Leave" icon={MessageSquare}>
                    <div className="form-grid">
                        <Toggle label="Welcome enabled" checked={config.welcome.enabled} onChange={value => patch("welcome.enabled", value)} />
                        <SelectField label="Welcome channel" value={config.welcome.channelId} onChange={value => patch("welcome.channelId", value)} options={serverChannels} />
                        <Field label="Welcome message" wide><TextArea rows={6} value={config.welcome.message} onChange={event => patch("welcome.message", event.target.value)} /></Field>
                        <Toggle label="Leave enabled" checked={config.leave.enabled} onChange={value => patch("leave.enabled", value)} />
                        <SelectField label="Leave channel" value={config.leave.channelId} onChange={value => patch("leave.channelId", value)} options={serverChannels} />
                        <Field label="Leave message" wide><TextArea rows={3} value={config.leave.message} onChange={event => patch("leave.message", event.target.value)} /></Field>
                    </div>
                </SectionCard>
            </div>
        );
    }

    function updateReactionOption(groupIndex, optionIndex, patchValue) {
        setConfig(current => {
            const next = clone(current);
            next.reactionRoles[groupIndex].options[optionIndex] = {
                ...next.reactionRoles[groupIndex].options[optionIndex],
                ...patchValue
            };
            return next;
        });
    }

    function addReactionOption(groupIndex) {
        setConfig(current => {
            const next = clone(current);
            next.reactionRoles[groupIndex].options.push({ emoji: "\u2705", roleId: "", label: "" });
            return next;
        });
    }

    function removeReactionOption(groupIndex, optionIndex) {
        setConfig(current => {
            const next = clone(current);
            next.reactionRoles[groupIndex].options.splice(optionIndex, 1);
            return next;
        });
    }
}

function formatDate(value) {
    if (!value) return "-";
    return new Date(value).toLocaleString();
}

function LogList({ items, type }) {
    if (!items.length) return <div className="empty-state">No entries yet.</div>;

    return (
        <div className="log-list">
            {items.map(item => {
                const title = type === "recruitment"
                    ? `${item.outcome === "accepted" ? item.team : "Rejected"} - ${item.applicantTag || item.applicantId}`
                    : item.title;
                const message = type === "recruitment"
                    ? [
                        `Closed by ${item.closedByTag || item.closedById}`,
                        item.licenseAnalysis?.inGameName ? `IGN: ${item.licenseAnalysis.inGameName}` : "",
                        item.licenseAnalysis?.sourceTeam ? `From: ${item.licenseAnalysis.sourceTeam}` : ""
                    ].filter(Boolean).join(" | ")
                    : item.message;

                return (
                    <article className="log-item" key={item.id || `${item.threadId}-${item.closedAt}`}>
                        <div>
                            <strong>{title}</strong>
                            <span>{message}</span>
                        </div>
                        <time>{formatDate(item.closedAt || item.createdAt)}</time>
                    </article>
                );
            })}
        </div>
    );
}
