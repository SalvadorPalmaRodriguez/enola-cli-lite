// Enola Web Dashboard — SPA logic (vanilla JS)
(function () {
    'use strict';

    let TOKEN = '';

    // ── Token overlay ──────────────────────────────────────────────────────
    const overlay = document.getElementById('token-overlay');
    const tokenInput = document.getElementById('token-input');
    const tokenSubmit = document.getElementById('token-submit');
    const tokenError = document.getElementById('token-error');
    const app = document.getElementById('app');

    tokenSubmit.addEventListener('click', tryConnect);
    tokenInput.addEventListener('keydown', function (e) {
        if (e.key === 'Enter') tryConnect();
    });

    async function tryConnect() {
        TOKEN = tokenInput.value.trim();
        if (!TOKEN) { tokenError.textContent = 'Token required'; return; }
        try {
            const res = await apiGet('/api/status');
            if (res.ok) {
                const data = await res.json();
                document.getElementById('version-badge').textContent = 'v' + data.version;
                overlay.style.display = 'none';
                app.style.display = 'block';
                init();
            } else {
                tokenError.textContent = 'Invalid token';
            }
        } catch (e) {
            tokenError.textContent = 'Connection failed: ' + e.message;
        }
    }

    // ── API helpers ────────────────────────────────────────────────────────
    async function apiGet(url) {
        return fetch(url, { headers: { 'Authorization': TOKEN } });
    }
    async function apiPost(url, body) {
        return fetch(url, {
            method: 'POST',
            headers: { 'Authorization': TOKEN, 'Content-Type': 'application/json' },
            body: body ? JSON.stringify(body) : undefined,
        });
    }

    async function apiGetJson(url) {
        const res = await apiGet(url);
        if (!res.ok) {
            const err = await res.json().catch(() => ({ error: 'Request failed' }));
            throw new Error(err.error || res.statusText);
        }
        return res.json();
    }

    async function apiPostJson(url, body) {
        const res = await apiPost(url, body);
        if (!res.ok) {
            const err = await res.json().catch(() => ({ error: 'Request failed' }));
            throw new Error(err.error || res.statusText);
        }
        return res.json().catch(() => null);
    }

    // ── Toast ──────────────────────────────────────────────────────────────
    let toastTimer;
    function toast(msg, isError) {
        const el = document.getElementById('toast');
        el.textContent = msg;
        el.style.borderColor = isError ? 'var(--danger)' : 'var(--accent)';
        el.style.color = isError ? 'var(--danger)' : 'var(--accent)';
        el.style.display = 'block';
        clearTimeout(toastTimer);
        toastTimer = setTimeout(() => { el.style.display = 'none'; }, 3000);
    }

    // ── Infrastructure helpers ─────────────────────────────────────────────
    function escapeHtml(s) {
        if (s == null) return '';
        return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
    }

    function statusBadge(status) {
        const s = String(status || '').toLowerCase();
        let cls = 'badge-dim';
        if (['running','active','ok','up','healthy'].includes(s)) cls = 'badge-ok';
        else if (['stopped','inactive','error','down','failed','exited'].includes(s)) cls = 'badge-err';
        else if (['published','pending','warning'].includes(s)) cls = 'badge-warn';
        return '<span class="badge ' + cls + '">' + escapeHtml(status) + '</span>';
    }

    async function withLoading(btn, asyncFn) {
        if (!btn) return asyncFn();
        const orig = btn.textContent;
        btn.classList.add('btn-loading');
        btn.disabled = true;
        try {
            return await asyncFn();
        } finally {
            btn.classList.remove('btn-loading');
            btn.disabled = false;
        }
    }

    function loadingPulse(msg) {
        return '<div class="loading-pulse">' + escapeHtml(msg || 'Loading...') + '</div>';
    }

    function emptyState(msg) {
        return '<div class="empty-state">' + escapeHtml(msg || 'No data.') + '</div>';
    }

    function copyToClipboard(text, btn) {
        navigator.clipboard.writeText(text).then(() => {
            if (btn) {
                const orig = btn.textContent;
                btn.textContent = '✓ Copied';
                btn.classList.add('copied');
                setTimeout(() => { btn.textContent = orig; btn.classList.remove('copied'); }, 1500);
            }
            toast('Copied to clipboard');
        }).catch(() => toast('Copy failed', true));
    }

    function copyBtn(text, label) {
        label = label || 'Copy';
        return '<button class="copy-btn" onclick="copyToClipboard(\'' + text.replace(/'/g,"\\'") + '\', this)">' + label + '</button>';
    }
    window.copyToClipboard = copyToClipboard;

    // ── Confirm modal ──────────────────────────────────────────────────────
    let confirmCallback = null;
    function showConfirm(title, message, onYes) {
        document.getElementById('confirm-title').textContent = title;
        document.getElementById('confirm-message').textContent = message;
        document.getElementById('confirm-modal').style.display = 'flex';
        confirmCallback = onYes;
    }
    window._confirmYes = function() {
        document.getElementById('confirm-modal').style.display = 'none';
        if (confirmCallback) { const cb = confirmCallback; confirmCallback = null; cb(); }
    };
    window._confirmNo = function() {
        document.getElementById('confirm-modal').style.display = 'none';
        confirmCallback = null;
    };

    // ── Tab switching ──────────────────────────────────────────────────────
    document.querySelectorAll('.tab').forEach(tab => {
        tab.addEventListener('click', () => {
            document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
            document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
            tab.classList.add('active');
            document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
        });
    });

    document.getElementById('refresh-btn').addEventListener('click', function() {
        this.classList.add('spinning');
        loadAll();
        toast('Refreshed');
        setTimeout(() => this.classList.remove('spinning'), 800);
    });

    // ── Init ───────────────────────────────────────────────────────────────
    async function init() { await loadConsoleCommands(); loadAll(); }

    async function loadAll() {
        loadServices();
        loadTor();
        loadGit();
        loadWp();
        loadFiles();
        loadPorts();
        loadFirewall();
        loadAppArmor();
        loadVpn();
        loadLogSources();
    }

    // ── Services ───────────────────────────────────────────────────────────
    async function loadServices() {
        const el = document.getElementById('services-table');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/services');
            if (!data.length) { el.innerHTML = emptyState('No services found.'); return; }
            let html = '<table><thead><tr><th>Type</th><th>Name</th><th>Status</th><th>Port</th><th>Onion</th></tr></thead><tbody>';
            data.forEach(s => {
                html += `<tr><td>${escapeHtml(s.service_type)}</td><td>${escapeHtml(s.name)}</td><td>${statusBadge(s.status)}</td><td>${s.port || '-'}</td><td>${escapeHtml(s.onion) || '-'}</td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('services-table').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    // ── Tor ────────────────────────────────────────────────────────────────
    let torServices = [];
    async function loadTor() {
        const el = document.getElementById('tor-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/tor');
            torServices = data.map(t => t.name);
            updateTorAuthDropdown();
            if (!data.length) { el.innerHTML = emptyState('No Tor services.'); return; }
            let html = '<table><thead><tr><th>Name</th><th>Type</th><th>Onion Address</th><th>Port</th><th>Active</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(t => {
                const active = t.active ? statusBadge('active') : statusBadge('inactive');
                html += `<tr><td>${escapeHtml(t.name)}</td><td>${escapeHtml(t.service_type || '-')}</td><td>${escapeHtml(t.hostname)}</td><td>${t.ports ? t.ports.map(p => p[0]).join(', ') : t.virtual_port}</td><td>${active}</td>
                    <td>
                        <button class="btn-sm" onclick="torAction('${escapeHtml(t.name)}','start')">Start</button>
                        <button class="btn-sm" onclick="torAction('${escapeHtml(t.name)}','stop')">Stop</button>
                        <button class="btn-sm" onclick="torEdit('${escapeHtml(t.name)}')">Edit</button>
                        <button class="btn-sm" onclick="torRotate('${escapeHtml(t.name)}')">Rotate</button>
                        <button class="btn-sm btn-danger" onclick="torAction('${escapeHtml(t.name)}','remove')">Remove</button>
                    </td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('tor-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    window.torRotate = async function (name) {
        try {
            const result = await apiPostJson('/api/tor/' + name + '/rotate');
            toast('Rotated: ' + (result || 'ok'));
            loadTor();
        } catch (e) { toast(e.message, true); }
    };

    window.torCreate = async function () {
        const name = document.getElementById('tor-create-name').value.trim();
        const service_type = document.getElementById('tor-create-type').value;
        const vport = parseInt(document.getElementById('tor-create-vport').value) || 80;
        const tportVal = document.getElementById('tor-create-tport').value;
        const target_port = tportVal ? parseInt(tportVal) : null;
        const ssl = document.getElementById('tor-create-ssl').checked;
        if (!name) { toast('Name required', true); return; }
        try {
            const result = await apiPostJson('/api/tor/create', { name, service_type, virtual_port: vport, target_port, ssl });
            toast('Created: ' + (result || 'ok'));
            loadTor();
        } catch (e) { toast(e.message, true); }
    };

    window.torAction = async function (name, action) {
        try {
            if (action === 'remove') {
                showConfirm('Remove Tor Service', 'Remove "' + name + '"? This cannot be undone.', () => torActionDo(name, action));
                return;
            }
            await torActionDo(name, action);
        } catch (e) { toast(e.message, true); }
    };
    async function torActionDo(name, action) {
        try {
            await apiPostJson('/api/tor/' + name + '/' + action);
            toast(action + ': ' + name);
            loadTor();
        } catch (e) { toast(e.message, true); }
    };

    let torEditTarget = null;

    window.torEdit = async function (name) {
        torEditTarget = name;
        try {
            const detail = await apiGetJson('/api/tor/' + encodeURIComponent(name) + '/detail');
            document.getElementById('tor-edit-name').textContent = name;
            const p = detail.ports && detail.ports[0] ? detail.ports[0] : { virtual_port: 80, nginx_port: 0, backend_port: 8080 };
            document.getElementById('tor-edit-vport').value = p.virtual_port;
            document.getElementById('tor-edit-nport').value = p.nginx_port;
            document.getElementById('tor-edit-tport').value = p.backend_port;
            document.getElementById('tor-edit-auto').checked = false;
            document.getElementById('tor-edit-modal').style.display = 'flex';
        } catch (e) { toast(e.message, true); }
    };

    window.torEditCancel = function () {
        document.getElementById('tor-edit-modal').style.display = 'none';
        torEditTarget = null;
    };

    window.torEditSave = async function () {
        if (!torEditTarget) return;
        const vport = parseInt(document.getElementById('tor-edit-vport').value);
        const nport = parseInt(document.getElementById('tor-edit-nport').value);
        const tport = parseInt(document.getElementById('tor-edit-tport').value);
        const auto = document.getElementById('tor-edit-auto').checked;

        const req = { virtual_port: vport, nginx_port: nport, target_port: tport, auto_ports: auto };

        try {
            const result = await apiPostJson('/api/tor/' + encodeURIComponent(torEditTarget) + '/edit', req);
            if (result.warning && !result.applied) {
                if (confirm(result.warning)) {
                    req.force = true;
                    await apiPostJson('/api/tor/' + encodeURIComponent(torEditTarget) + '/edit', req);
                    toast('Servicio editado');
                } else {
                    toast('Edición cancelada');
                    return;
                }
            } else {
                toast('Servicio editado');
            }
            torEditCancel();
            loadTor();
        } catch (e) { toast(e.message, true); }
    };

    // ── Tor Auth ──────────────────────────────────────────────────────────
    function updateTorAuthDropdown() {
        const sel = document.getElementById('tor-auth-service');
        if (!sel) return;
        const current = sel.value;
        sel.innerHTML = '<option value="">-- Select service --</option>' +
            torServices.map(s => '<option value="' + escapeHtml(s) + '">' + escapeHtml(s) + '</option>').join('');
        if (current && torServices.includes(current)) sel.value = current;
    }

    window.torAuthLoadClients = async function() {
        const service = document.getElementById('tor-auth-service').value;
        const el = document.getElementById('tor-auth-clients');
        if (!service) { el.innerHTML = emptyState('Select a service first.'); return; }
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/tor/auth/' + encodeURIComponent(service) + '/list');
            if (!data.length) { el.innerHTML = emptyState('No authorized clients.'); return; }
            let html = '<table><thead><tr><th>Client</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(c => {
                html += `<tr><td>${escapeHtml(c)}</td>
                    <td>
                        <button class="btn-sm btn-danger" onclick="torAuthRevokeClient('${escapeHtml(service)}','${escapeHtml(c)}')">Revoke</button>
                    </td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { el.innerHTML = '<p class="error">' + escapeHtml(e.message) + '</p>'; }
    };

    window.torAuthRevokeClient = async function(service, client) {
        showConfirm('Revoke Client', 'Revoke client "' + client + '" from "' + service + '"?', async () => {
            try {
                await apiPostJson('/api/tor/auth/' + encodeURIComponent(service) + '/revoke', { client });
                toast('Client revoked: ' + client);
                torAuthLoadClients();
            } catch (e) { toast(e.message, true); }
        });
    };

    window.torAuthList = async function () {
        torAuthLoadClients();
    };

    window.torAuthEnable = async function () {
        const service = document.getElementById('tor-auth-service').value;
        if (!service) { toast('Select a service first', true); return; }
        try {
            await apiPostJson('/api/tor/auth/' + encodeURIComponent(service) + '/enable');
            toast('Auth enabled for ' + service);
        } catch (e) { toast(e.message, true); }
    };

    window.torAuthDisable = async function () {
        const service = document.getElementById('tor-auth-service').value;
        if (!service) { toast('Select a service first', true); return; }
        try {
            await apiPostJson('/api/tor/auth/' + encodeURIComponent(service) + '/disable');
            toast('Auth disabled for ' + service);
        } catch (e) { toast(e.message, true); }
    };

    window.torAuthGenerate = async function () {
        const client = document.getElementById('tor-auth-gen-client').value.trim();
        if (!client) { toast('Client name required', true); return; }
        try {
            const data = await apiPostJson('/api/tor/auth/generate', { client });
            const el = document.getElementById('tor-auth-output');
            el.innerHTML = '<div class="key-display">Public key: ' + escapeHtml(data.public_key) +
                copyBtn(data.public_key, 'Copy') + '</div>' +
                '<div class="key-display">Private key: ' + escapeHtml(data.private_key) +
                copyBtn(data.private_key, 'Copy') + '</div>' +
                '<p class="hint">' + escapeHtml(data.message) + '</p>' +
                '<p class="hint">⚠️ Send the PRIVATE key to the client securely. The PUBLIC key stays on the server.</p>';
        } catch (e) { toast(e.message, true); }
    };

    window.torAuthAdd = async function () {
        const service = document.getElementById('tor-auth-service').value;
        const client = document.getElementById('tor-auth-add-client').value.trim();
        const pubkey = document.getElementById('tor-auth-add-pubkey').value.trim();
        if (!service || !client || !pubkey) { toast('Service, client, and pubkey required', true); return; }
        try {
            const result = await apiPostJson('/api/tor/auth/' + encodeURIComponent(service) + '/add', { client, pubkey });
            toast(result || 'Client added');
            torAuthLoadClients();
        } catch (e) { toast(e.message, true); }
    };

    window.torAuthRevoke = async function () {
        const service = document.getElementById('tor-auth-service').value;
        const client = document.getElementById('tor-auth-revoke-client').value.trim();
        if (!service || !client) { toast('Service and client required', true); return; }
        showConfirm('Revoke Client', 'Revoke client "' + client + '" from "' + service + '"?', async () => {
            try {
                const result = await apiPostJson('/api/tor/auth/' + encodeURIComponent(service) + '/revoke', { client });
                toast(result || 'Client revoked');
                torAuthLoadClients();
            } catch (e) { toast(e.message, true); }
        });
    };

    window.torAuthRotate = async function () {
        const service = document.getElementById('tor-auth-service').value;
        if (!service) { toast('Select a service first', true); return; }
        try {
            const result = await apiPostJson('/api/tor/auth/' + encodeURIComponent(service) + '/rotate');
            if (result && result.public_key) {
                const el = document.getElementById('tor-auth-output');
                el.innerHTML = '<div class="key-display">New public key: ' + escapeHtml(result.public_key) +
                    copyBtn(result.public_key, 'Copy') + '</div>' +
                    '<div class="key-display">New private key: ' + escapeHtml(result.private_key) +
                    copyBtn(result.private_key, 'Copy') + '</div>' +
                    '<p class="hint">' + escapeHtml(result.message) + '</p>';
            } else {
                toast(result || 'Keys rotated');
            }
        } catch (e) { toast(e.message, true); }
    };

    // ── Git ────────────────────────────────────────────────────────────────
    let gitServers = [];
    async function loadGit() {
        const el = document.getElementById('git-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/git');
            gitServers = data.map(g => g.name);
            updateGitUserServerDropdown();
            if (!data.length) { el.innerHTML = emptyState('No Git servers.'); return; }
            let html = '<table><thead><tr><th>Name</th><th>Status</th><th>Ports</th><th>Onion</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(g => {
                html += `<tr><td>${escapeHtml(g.name)}</td><td>${statusBadge(g.status)}</td><td>${(g.ports || []).join(', ')}</td><td>${escapeHtml(g.onion_address) || '-'}</td>
                    <td>
                        <button class="btn-sm" onclick="gitAction('${escapeHtml(g.name)}','start')">Start</button>
                        <button class="btn-sm" onclick="gitAction('${escapeHtml(g.name)}','stop')">Stop</button>
                        <button class="btn-sm" onclick="gitAction('${escapeHtml(g.name)}','publish')">Publish</button>
                        <button class="btn-sm" onclick="gitAction('${escapeHtml(g.name)}','hide')">Hide</button>
                        <button class="btn-sm" onclick="gitEdit('${escapeHtml(g.name)}')">Edit</button>
                        <button class="btn-sm btn-danger" onclick="gitAction('${escapeHtml(g.name)}','delete')">Delete</button>
                    </td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('git-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    window.gitCreate = async function () {
        const name = document.getElementById('git-create-name').value.trim();
        if (!name) { toast('Name required', true); return; }
        const user = document.getElementById('git-create-user').value.trim();
        const pass = document.getElementById('git-create-pass').value;
        const ssl = document.getElementById('git-create-ssl').checked;
        const httpPortVal = document.getElementById('git-create-http-port').value;
        const sshPortVal = document.getElementById('git-create-ssh-port').value;
        const body = { name, ssl, admin_user: user || null, admin_pass: pass || null };
        if (httpPortVal) body.http_port = parseInt(httpPortVal);
        if (sshPortVal) body.ssh_port = parseInt(sshPortVal);
        try {
            const result = await apiPostJson('/api/git/create', body);
            toast('Created: ' + (result || 'ok'));
            loadGit();
        } catch (e) { toast(e.message, true); }
    };

    window.gitAction = async function (name, action) {
        try {
            if (action === 'delete') {
                showConfirm('Delete Git Server', 'Delete "' + name + '"? This will remove the container and all data.', () => gitActionDo(name, action));
                return;
            }
            await gitActionDo(name, action);
        } catch (e) { toast(e.message, true); }
    };
    async function gitActionDo(name, action) {
        try {
            if (action === 'publish') {
                const ssl = confirm('Publish with SSL? (Cancel = HTTP)');
                const result = await apiPostJson('/api/git/' + name + '/publish', { ssl });
                toast('Published: ' + (result || 'ok'));
            } else if (action === 'watcher') {
                const result = await apiPostJson('/api/git/watcher');
                toast('Watcher: ' + (result || 'ok'));
            } else {
                await apiPostJson('/api/git/' + name + '/' + action);
                toast(action + ': ok');
            }
            loadGit();
        } catch (e) { toast(e.message, true); }
    }

    function updateGitUserServerDropdown() {
        const sel = document.getElementById('git-user-server');
        if (!sel) return;
        const current = sel.value;
        sel.innerHTML = '<option value="">-- Select server --</option>' +
            gitServers.map(s => '<option value="' + escapeHtml(s) + '">' + escapeHtml(s) + '</option>').join('');
        if (current && gitServers.includes(current)) sel.value = current;
    };

    window.gitStatus = async function (name) {
        try {
            const data = await apiGetJson('/api/git/' + name + '/status');
            toast('Status: ' + JSON.stringify(data));
        } catch (e) { toast(e.message, true); }
    };

    window.gitRegistration = async function (name, enable) {
        try {
            await apiPostJson('/api/git/' + name + '/registration', { enable });
            toast('Registration ' + (enable ? 'enabled' : 'disabled'));
        } catch (e) { toast(e.message, true); }
    };

    let gitEditTarget = null;

    window.gitEdit = async function (name) {
        gitEditTarget = name;
        try {
            const data = await apiGetJson('/api/git');
            const g = data.find(x => x.name === name);
            document.getElementById('git-edit-name').textContent = name;
            document.getElementById('git-edit-http').value = g && g.http_port || '';
            document.getElementById('git-edit-https').value = '';
            document.getElementById('git-edit-ssh').value = g && g.ssh_port || '';
            document.getElementById('git-edit-auto').checked = false;
            document.getElementById('git-edit-modal').style.display = 'flex';
        } catch (e) { toast(e.message, true); }
    };

    window.gitEditCancel = function () {
        document.getElementById('git-edit-modal').style.display = 'none';
        gitEditTarget = null;
    };

    window.gitEditSave = async function () {
        if (!gitEditTarget) return;
        const httpPort = document.getElementById('git-edit-http').value;
        const httpsPort = document.getElementById('git-edit-https').value;
        const sshPort = document.getElementById('git-edit-ssh').value;
        const auto = document.getElementById('git-edit-auto').checked;
        const req = { auto_ports: auto };
        if (httpPort) req.http_port = parseInt(httpPort);
        if (httpsPort) req.https_port = parseInt(httpsPort);
        if (sshPort) req.ssh_port = parseInt(sshPort);
        try {
            const result = await apiPostJson('/api/git/' + gitEditTarget + '/edit', req);
            toast('Edited: ' + (result || 'ok'));
            gitEditCancel();
            loadGit();
        } catch (e) { toast(e.message, true); }
    };

    // ── Git User Management ────────────────────────────────────────────────
    window.gitUserList = async function () {
        const server = document.getElementById('git-user-server').value.trim();
        if (!server) { toast('Select a server first', true); return; }
        const admin_user = document.getElementById('git-user-admin').value.trim() || null;
        const admin_pass = document.getElementById('git-user-adminpass').value || null;
        const el = document.getElementById('git-user-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiPostJson('/api/git/user/list', { server, admin_user, admin_pass });
            if (!data.length) { el.innerHTML = emptyState('No users.'); return; }
            let html = '<table><thead><tr><th>Username</th><th>Email</th><th>Admin</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(u => {
                html += `<tr><td>${escapeHtml(u.username)}</td><td>${escapeHtml(u.email)}</td><td>${u.is_admin ? '✅' : '❌'}</td>
                    <td><button class="btn-sm btn-danger" onclick="gitUserDeleteByName('${escapeHtml(u.username)}')">Delete</button></td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('git-user-list').innerHTML = '<p class="error">' + escapeHtml(e.message) + '</p>'; }
    };

    window.gitUserCreate = async function () {
        const server = document.getElementById('git-user-server').value.trim();
        const username = document.getElementById('git-user-create-username').value.trim();
        const email = document.getElementById('git-user-create-email').value.trim();
        const password = document.getElementById('git-user-create-pass').value;
        const is_admin = document.getElementById('git-user-create-admin').checked;
        const admin_user = document.getElementById('git-user-admin').value.trim() || null;
        const admin_pass = document.getElementById('git-user-adminpass').value || null;
        if (!server || !username || !email || !password) { toast('Server, username, email, password required', true); return; }
        try {
            await apiPostJson('/api/git/user/create', { server, username, email, password, is_admin, admin_user, admin_pass });
            toast('User created: ' + username);
            gitUserList();
        } catch (e) { toast(e.message, true); }
    };

    window.gitUserDelete = async function () {
        const server = document.getElementById('git-user-server').value.trim();
        const username = document.getElementById('git-user-delete-username').value.trim();
        const admin_user = document.getElementById('git-user-admin').value.trim() || null;
        const admin_pass = document.getElementById('git-user-adminpass').value || null;
        if (!server || !username) { toast('Server and username required', true); return; }
        try {
            await apiPostJson('/api/git/user/delete', { server, username, admin_user, admin_pass });
            toast('User deleted: ' + username);
            gitUserList();
        } catch (e) { toast(e.message, true); }
    };

    window.gitUserDeleteByName = async function (username) {
        const server = document.getElementById('git-user-server').value.trim();
        const admin_user = document.getElementById('git-user-admin').value.trim() || null;
        const admin_pass = document.getElementById('git-user-adminpass').value || null;
        if (!server) { toast('Server name required', true); return; }
        try {
            await apiPostJson('/api/git/user/delete', { server, username, admin_user, admin_pass });
            toast('User deleted: ' + username);
            gitUserList();
        } catch (e) { toast(e.message, true); }
    };

    // ── WordPress ──────────────────────────────────────────────────────────
    async function loadWp() {
        const el = document.getElementById('wp-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/wp');
            if (!data.length) { el.innerHTML = emptyState('No WordPress sites.'); return; }
            let html = '<table><thead><tr><th>Name</th><th>Status</th><th>Port</th><th>Onion</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(w => {
                html += `<tr><td>${escapeHtml(w.name)}</td><td>${statusBadge(w.status)}</td><td>${w.port}</td><td>${escapeHtml(w.onion_address) || '-'}</td>
                    <td>
                        <button class="btn-sm" onclick="wpAction('${escapeHtml(w.name)}','start')">Start</button>
                        <button class="btn-sm" onclick="wpAction('${escapeHtml(w.name)}','stop')">Stop</button>
                        <button class="btn-sm" onclick="wpRestart('${escapeHtml(w.name)}')">Restart</button>
                        <button class="btn-sm" onclick="wpAction('${escapeHtml(w.name)}','publish')">Publish</button>
                        <button class="btn-sm" onclick="wpAction('${escapeHtml(w.name)}','hide')">Hide</button>
                        <button class="btn-sm" onclick="wpUpdate('${escapeHtml(w.name)}')">Update</button>
                        <button class="btn-sm" onclick="wpConfig('${escapeHtml(w.name)}')">Config</button>
                        <button class="btn-sm" onclick="wpStatus('${escapeHtml(w.name)}')">Status</button>
                        <button class="btn-sm" onclick="wpEdit('${escapeHtml(w.name)}')">Edit</button>
                        <button class="btn-sm btn-danger" onclick="wpAction('${escapeHtml(w.name)}','delete')">Delete</button>
                    </td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('wp-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    window.wpCreate = async function () {
        const name = document.getElementById('wp-create-name').value.trim();
        if (!name) { toast('Name required', true); return; }
        const portVal = document.getElementById('wp-create-port').value;
        const http_port = portVal ? parseInt(portVal) : null;
        try {
            const result = await apiPostJson('/api/wp/create', { name, http_port });
            toast('Created: ' + (result || 'ok'));
            loadWp();
        } catch (e) { toast(e.message, true); }
    };

    window.wpAction = async function (name, action) {
        try {
            if (action === 'delete') {
                showConfirm('Delete WordPress Site', 'Delete "' + name + '"? This will remove the container and all data.', () => wpActionDo(name, action));
                return;
            }
            await wpActionDo(name, action);
        } catch (e) { toast(e.message, true); }
    };
    async function wpActionDo(name, action) {
        try {
            if (action === 'publish') {
                const result = await apiPostJson('/api/wp/' + name + '/publish');
                toast('Published: ' + (result || 'ok'));
            } else {
                await apiPostJson('/api/wp/' + name + '/' + action);
                toast(action + ': ok');
            }
            loadWp();
        } catch (e) { toast(e.message, true); }
    };

    window.wpRestart = async function (name) {
        try { await apiPostJson('/api/wp/' + name + '/restart'); toast('Restarted: ' + name); loadWp(); }
        catch (e) { toast(e.message, true); }
    };

    window.wpUpdate = async function (name) {
        try { await apiPostJson('/api/wp/' + name + '/update'); toast('Updated: ' + name); }
        catch (e) { toast(e.message, true); }
    };

    window.wpConfig = async function (name) {
        try { const data = await apiGetJson('/api/wp/' + name + '/config'); toast('Config: ' + data); }
        catch (e) { toast(e.message, true); }
    };

    window.wpStatus = async function (name) {
        try { const data = await apiGetJson('/api/wp/' + name + '/status'); toast('Status: ' + JSON.stringify(data)); }
        catch (e) { toast(e.message, true); }
    };

    let wpEditTarget = null;

    window.wpEdit = async function (name) {
        wpEditTarget = name;
        try {
            const config = await apiGetJson('/api/wp/' + name + '/config');
            document.getElementById('wp-edit-name').textContent = name;
            document.getElementById('wp-edit-http').value = config.http_port || '';
            document.getElementById('wp-edit-https').value = config.https_port || '';
            document.getElementById('wp-edit-ssl').value = String(config.ssl || false);
            document.getElementById('wp-edit-auto').checked = false;
            document.getElementById('wp-edit-modal').style.display = 'flex';
        } catch (e) {
            document.getElementById('wp-edit-name').textContent = name;
            document.getElementById('wp-edit-http').value = '';
            document.getElementById('wp-edit-https').value = '';
            document.getElementById('wp-edit-ssl').value = 'false';
            document.getElementById('wp-edit-auto').checked = false;
            document.getElementById('wp-edit-modal').style.display = 'flex';
        }
    };

    window.wpEditCancel = function () {
        document.getElementById('wp-edit-modal').style.display = 'none';
        wpEditTarget = null;
    };

    window.wpEditSave = async function () {
        if (!wpEditTarget) return;
        const httpPort = document.getElementById('wp-edit-http').value;
        const httpsPort = document.getElementById('wp-edit-https').value;
        const ssl = document.getElementById('wp-edit-ssl').value === 'true';
        const auto = document.getElementById('wp-edit-auto').checked;
        const req = { auto_ports: auto, ssl };
        if (httpPort) req.http_port = parseInt(httpPort);
        if (httpsPort) req.https_port = parseInt(httpsPort);
        try { const result = await apiPostJson('/api/wp/' + wpEditTarget + '/edit', req); toast('Edited: ' + (result || 'ok')); wpEditCancel(); loadWp(); }
        catch (e) { toast(e.message, true); }
    };

    // ── CMS ────────────────────────────────────────────────────────────────
    window.cmsList = async function () {
        const type = document.getElementById('cms-type').value;
        const el = document.getElementById('cms-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/cms/' + type + '/list');
            if (!data.length) { el.innerHTML = emptyState('No ' + type + ' instances.'); return; }
            let html = '<table><thead><tr><th>Name</th><th>Status</th><th>Port</th><th>Onion</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(c => {
                const name = c.name || c;
                html += `<tr><td>${escapeHtml(name)}</td><td>${statusBadge(c.status || '-')}</td><td>${c.http_port || '-'}</td><td>${escapeHtml(c.onion_address) || '-'}</td>
                    <td>
                        <button class="btn-sm" onclick="cmsAction('${type}','${escapeHtml(name)}','start')">Start</button>
                        <button class="btn-sm" onclick="cmsAction('${type}','${escapeHtml(name)}','stop')">Stop</button>
                        <button class="btn-sm" onclick="cmsAction('${type}','${escapeHtml(name)}','publish')">Publish</button>
                        <button class="btn-sm" onclick="cmsAction('${type}','${escapeHtml(name)}','hide')">Hide</button>
                        <button class="btn-sm" onclick="cmsStatus('${type}','${escapeHtml(name)}')">Status</button>
                        <button class="btn-sm" onclick="cmsEdit('${type}','${escapeHtml(name)}')">Edit</button>
                        <button class="btn-sm btn-danger" onclick="cmsAction('${type}','${escapeHtml(name)}','delete')">Delete</button>
                    </td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('cms-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    };

    window.cmsCreate = async function () {
        const type = document.getElementById('cms-type').value;
        const name = document.getElementById('cms-create-name').value.trim();
        if (!name) { toast('Instance name required', true); return; }
        const portVal = document.getElementById('cms-create-port').value;
        if (!portVal) { toast('HTTP port is required for CMS instances', true); return; }
        const http_port = parseInt(portVal);
        try {
            const result = await apiPostJson('/api/cms/' + type + '/create', { name, http_port });
            toast('Created: ' + (result || 'ok'));
            cmsList();
        } catch (e) { toast(e.message, true); }
    };

    window.cmsAction = async function (type, name, action) {
        try {
            if (action === 'delete') {
                showConfirm('Delete CMS Instance', 'Delete "' + type + '/' + name + '"? This will remove the container.', () => cmsActionDo(type, name, action));
                return;
            }
            await cmsActionDo(type, name, action);
        } catch (e) { toast(e.message, true); }
    };
    async function cmsActionDo(type, name, action) {
        try {
            if (action === 'publish') {
                const result = await apiPostJson('/api/cms/' + type + '/' + name + '/publish');
                toast('Published: ' + (result || 'ok'));
            } else if (action === 'delete') {
                await apiPostJson('/api/cms/' + type + '/' + name + '/delete', { force: true });
                toast('Deleted: ' + name);
            } else {
                await apiPostJson('/api/cms/' + type + '/' + name + '/' + action);
                toast(action + ': ok');
            }
            cmsList();
        } catch (e) { toast(e.message, true); }
    };

    window.strapiBuildImage = async function () {
        try {
            const result = await apiPostJson('/api/cms/strapi/build-image', {});
            toast('Strapi image: ' + (result || 'ok'));
        } catch (e) { toast(e.message, true); }
    };

    window.cmsStatus = async function (type, name) {
        try { const data = await apiGetJson('/api/cms/' + type + '/' + name + '/status'); toast('Status: ' + JSON.stringify(data)); }
        catch (e) { toast(e.message, true); }
    };

    let cmsEditTarget = null;
    window.cmsEdit = async function (type, name) {
        cmsEditTarget = { type, name };
        document.getElementById('cms-edit-name').textContent = type + '/' + name;
        document.getElementById('cms-edit-port').value = '';
        document.getElementById('cms-edit-modal').style.display = 'flex';
    };
    window.cmsEditCancel = function () {
        document.getElementById('cms-edit-modal').style.display = 'none';
        cmsEditTarget = null;
    };
    window.cmsEditSave = async function () {
        if (!cmsEditTarget) return;
        const portStr = document.getElementById('cms-edit-port').value;
        if (!portStr) { toast('Port required', true); return; }
        const http_port = parseInt(portStr);
        try {
            const result = await apiPostJson('/api/cms/' + cmsEditTarget.type + '/' + cmsEditTarget.name + '/edit', { http_port });
            toast('Edited: ' + (result || 'ok'));
            cmsEditCancel();
            cmsList();
        } catch (e) { toast(e.message, true); }
    };

    // ── Files ──────────────────────────────────────────────────────────────
    async function loadFiles() {
        const el = document.getElementById('files-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/files');
            if (!data.length) { el.innerHTML = emptyState('No file shares.'); return; }
            let html = '<table><thead><tr><th>Name</th><th>Onion</th><th>Path</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(f => {
                const pathDisplay = escapeHtml(f.share_path) || '<span class="badge badge-dim">—</span>';
                html += `<tr><td>${escapeHtml(f.name)}</td><td>${escapeHtml(f.hostname)}</td><td title="${escapeHtml(f.share_path || '')}">${pathDisplay}</td>
                    <td>
                        <button class="btn-sm" onclick="filesEdit('${escapeHtml(f.name)}')">Edit</button>
                        <button class="btn-sm" onclick="filesFixPerms('${escapeHtml(f.name)}')">Fix Perms</button>
                        <button class="btn-sm btn-danger" onclick="filesDelete('${escapeHtml(f.name)}')">Delete</button>
                    </td></tr>`;
            });
            html += '</tbody></table>';
            const hintPath = data[0].share_path || ('/srv/enola-files/' + data[0].name);
            html += '<div class="hint">Copy files to share: <code>sudo cp your_files/* ' + escapeHtml(hintPath) + '/</code></div>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('files-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    window.filesCreate = async function () {
        const name = document.getElementById('files-create-name').value.trim();
        if (!name) { toast('Name required', true); return; }
        const auth = document.getElementById('files-create-auth').checked;
        const ssl = document.getElementById('files-create-ssl').checked;
        const body = { name, auth, ssl };
        try {
            const result = await apiPostJson('/api/files/create', body);
            toast('Created: ' + (result || 'ok'));
            loadFiles();
        } catch (e) { toast(e.message, true); }
    };

    window.filesDelete = async function (name) {
        try {
            await apiPostJson('/api/files/' + name + '/delete');
            toast('Deleted: ' + name);
            loadFiles();
        } catch (e) { toast(e.message, true); }
    };

    let filesEditTarget = null;

    window.filesEdit = async function (name) {
        filesEditTarget = name;
        document.getElementById('files-edit-name').textContent = name;
        document.getElementById('files-edit-port').value = '';
        document.getElementById('files-edit-modal').style.display = 'flex';
    };

    window.filesEditCancel = function () {
        document.getElementById('files-edit-modal').style.display = 'none';
        filesEditTarget = null;
    };

    window.filesEditSave = async function () {
        if (!filesEditTarget) return;
        const portVal = document.getElementById('files-edit-port').value;
        const req = {};
        if (portVal) req.port = parseInt(portVal);
        try {
            const result = await apiPostJson('/api/files/' + filesEditTarget + '/edit', req);
            toast('Edited: ' + (result || 'ok'));
            filesEditCancel();
            loadFiles();
        } catch (e) { toast(e.message, true); }
    };

    window.filesFixPerms = async function (name) {
        try {
            await apiPostJson('/api/files/' + name + '/fix-perms');
            toast('Permissions fixed: ' + name);
        } catch (e) { toast(e.message, true); }
    };

    // ── Ports ──────────────────────────────────────────────────────────────
    async function loadPorts() {
        const el = document.getElementById('ports-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/ports');
            if (!data.length) { el.innerHTML = emptyState('No port entries.'); return; }
            let html = '<table><thead><tr><th>Service</th><th>Type</th><th>Port</th><th>Bind</th></tr></thead><tbody>';
            data.forEach(p => {
                html += `<tr><td>${escapeHtml(p.service)}</td><td>${escapeHtml(p.service_type)}</td><td>${p.port}</td><td>${escapeHtml(p.bind_address || '127.0.0.1')}</td></tr>`;
            });
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('ports-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    // ── Firewall ───────────────────────────────────────────────────────────
    async function loadFirewall() {
        const el = document.getElementById('firewall-status');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/firewall/status');
            let html = `<p>Active: ${statusBadge(data.active ? 'active' : 'inactive')}</p>`;
            html += `<p>Default Incoming: ${escapeHtml(data.default_incoming)}</p>`;
            html += `<p>Default Outgoing: ${escapeHtml(data.default_outgoing)}</p>`;
            html += `<p>Rules: ${data.rules.length}</p>`;
            html += `<p>Docker-User: ${data.docker_user_configured ? '✅' : '❌'}</p>`;
            if (data.rules.length) {
                html += '<table><thead><tr><th>Port</th><th>Proto</th><th>From</th><th>Action</th></tr></thead><tbody>';
                data.rules.forEach(r => {
                    html += `<tr><td>${r.port}</td><td>${escapeHtml(r.protocol)}</td><td>${escapeHtml(r.from || 'any')}</td><td>${escapeHtml(r.action)}</td></tr>`;
                });
                html += '</tbody></table>';
            }
            el.innerHTML = html;
        } catch (e) { document.getElementById('firewall-status').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    window.firewallSetup = async function () {
        const sshPortVal = document.getElementById('fw-setup-ssh-port').value;
        const body = {};
        if (sshPortVal) body.ssh_port = parseInt(sshPortVal);
        try {
            const result = await apiPostJson('/api/firewall/setup', body);
            toast('Firewall setup: ' + (result || 'ok'));
            loadFirewall();
        } catch (e) { toast(e.message, true); }
    };

    window.firewallAllow = async function () {
        const port = parseInt(document.getElementById('fw-allow-port').value);
        const proto = document.getElementById('fw-allow-proto').value;
        const from = document.getElementById('fw-allow-from').value.trim() || null;
        if (!port) { toast('Port required', true); return; }
        try {
            const result = await apiPostJson('/api/firewall/allow', { port, proto, from });
            toast(result || 'Allowed');
            loadFirewall();
        } catch (e) { toast(e.message, true); }
    };

    window.firewallDeny = async function () {
        const port = parseInt(document.getElementById('fw-deny-port').value);
        const proto = document.getElementById('fw-deny-proto').value;
        if (!port) { toast('Port required', true); return; }
        try {
            const result = await apiPostJson('/api/firewall/deny', { port, proto });
            toast(result || 'Denied');
            loadFirewall();
        } catch (e) { toast(e.message, true); }
    };

    // ── AppArmor ───────────────────────────────────────────────────────────
    async function loadAppArmor() {
        const el = document.getElementById('apparmor-status');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/apparmor/status');
            let html = `<p>Installed: ${data.installed ? '✅' : '❌'}</p>`;
            html += `<p>Enabled: ${data.enabled ? '✅' : '❌'}</p>`;
            html += `<p>Profiles: ${data.profiles.length}</p>`;
            html += `<p>Violations (24h): ${data.recent_violations.length}</p>`;
            if (data.profiles.length) {
                html += '<table><thead><tr><th>Name</th><th>Mode</th><th>Type</th></tr></thead><tbody>';
                data.profiles.forEach(p => {
                    html += `<tr><td>${escapeHtml(p.name)}</td><td>${escapeHtml(p.mode)}</td><td>${escapeHtml(p.service_type)}</td></tr>`;
                });
                html += '</tbody></table>';
            }
            el.innerHTML = html;
        } catch (e) { document.getElementById('apparmor-status').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    window.apparmorSetup = async function () {
        const mode = document.getElementById('aa-mode').value;
        const force = document.getElementById('aa-setup-force').checked;
        try {
            const result = await apiPostJson('/api/apparmor/setup', { mode: mode, force });
            toast('AppArmor setup: ' + (result || 'ok'));
            loadAppArmor();
        } catch (e) { toast(e.message, true); }
    };

    window.apparmorMode = async function () {
        const mode = document.getElementById('aa-mode').value;
        const profile = document.getElementById('aa-profile').value.trim() || null;
        try {
            const result = await apiPostJson('/api/apparmor/mode', { mode, profile });
            toast(result || 'Mode set');
            loadAppArmor();
        } catch (e) { toast(e.message, true); }
    };

    // ── VPN ────────────────────────────────────────────────────────────────
    let vpnInterfaces = [];
    async function loadVpn() {
        const el = document.getElementById('vpn-list');
        el.innerHTML = loadingPulse();
        try {
            const data = await apiGetJson('/api/vpn/list');
            vpnInterfaces = data || [];
            updateVpnPeerDropdowns();
            if (!data.length) { el.innerHTML = emptyState('No VPN interfaces.'); return; }
            let html = '<table><thead><tr><th>Interface</th><th>Status</th><th>Actions</th></tr></thead><tbody>';
            for (const iface of data) {
                let status = '-';
                try {
                    const st = await apiGetJson('/api/vpn/status/' + iface);
                    status = 'listening:' + st.listen_port + ' peers:' + st.peers.length;
                } catch (e) { status = 'error'; }
                html += `<tr><td>${escapeHtml(iface)}</td><td>${escapeHtml(status)}</td>
                    <td>
                        <button class="btn-sm" onclick="vpnAction('${escapeHtml(iface)}','start')">Start</button>
                        <button class="btn-sm" onclick="vpnAction('${escapeHtml(iface)}','stop')">Stop</button>
                        <button class="btn-sm btn-danger" onclick="vpnAction('${escapeHtml(iface)}','delete')">Delete</button>
                    </td></tr>`;
            }
            html += '</tbody></table>';
            el.innerHTML = html;
        } catch (e) { document.getElementById('vpn-list').innerHTML = '<p class="error">Error: ' + escapeHtml(e.message) + '</p>'; }
    }

    function updateVpnPeerDropdowns() {
        const ifaces = vpnInterfaces.map(i => '<option value="' + escapeHtml(i) + '">' + escapeHtml(i) + '</option>').join('');
        ['vpn-peer-iface', 'vpn-peer-addpub-iface', 'vpn-peer-remove-iface'].forEach(id => {
            const sel = document.getElementById(id);
            if (!sel) return;
            const current = sel.value;
            sel.innerHTML = '<option value="">-- Select --</option>' + ifaces;
            if (current) sel.value = current;
        });
    }

    window.vpnCreate = async function () {
        const iface = document.getElementById('vpn-create-name').value.trim();
        if (!iface) { toast('Interface name required', true); return; }
        const portVal = document.getElementById('vpn-create-port').value;
        const port = portVal ? parseInt(portVal) : null;
        const subnet = document.getElementById('vpn-create-subnet').value.trim() || null;
        const autostart = document.getElementById('vpn-create-autostart').checked;
        try {
            const result = await apiPostJson('/api/vpn/create', { interface: iface, port, subnet, autostart });
            toast('Created: ' + (result || 'ok'));
            loadVpn();
        } catch (e) { toast(e.message, true); }
    };

    window.vpnAction = async function (iface, action) {
        try {
            if (action === 'delete') {
                showConfirm('Delete VPN Interface', 'Delete VPN "' + iface + '"? This will remove the interface and all peers.', () => vpnActionDo(iface, action));
                return;
            }
            await vpnActionDo(iface, action);
        } catch (e) { toast(e.message, true); }
    };
    async function vpnActionDo(iface, action) {
        try {
            if (action === 'delete') {
                await apiPostJson('/api/vpn/' + iface + '/delete', { sync_firewall: false });
            } else {
                await apiPostJson('/api/vpn/' + iface + '/' + action);
            }
            toast(action + ': ok');
            loadVpn();
        } catch (e) { toast(e.message, true); }
    };

    window.vpnPeerAdd = async function () {
        const iface = document.getElementById('vpn-peer-iface').value.trim();
        const peer_name = document.getElementById('vpn-peer-name').value.trim();
        const endpoint = document.getElementById('vpn-peer-endpoint').value.trim() || null;
        const dns = document.getElementById('vpn-peer-dns').value.trim() || null;
        const psk = document.getElementById('vpn-peer-psk').checked;
        const ip = document.getElementById('vpn-peer-ip').value.trim() || null;
        if (!iface || !peer_name) { toast('Interface and peer name required', true); return; }
        try {
            const result = await apiPostJson('/api/vpn/peer/add', { interface: iface, peer_name, endpoint, psk, dns, ip });
            document.getElementById('vpn-peer-output').textContent = result;
            toast('Peer added');
        } catch (e) { toast(e.message, true); }
    };

    window.vpnPeerAddPubkey = async function () {
        const iface = document.getElementById('vpn-peer-addpub-iface').value.trim();
        const peer_name = document.getElementById('vpn-peer-addpub-name').value.trim();
        const public_key = document.getElementById('vpn-peer-addpub-key').value.trim();
        const ip = document.getElementById('vpn-peer-addpub-ip').value.trim();
        if (!iface || !peer_name || !public_key || !ip) { toast('All fields required', true); return; }
        try {
            const result = await apiPostJson('/api/vpn/peer/add-pubkey', { interface: iface, peer_name, public_key, ip });
            document.getElementById('vpn-peer-output').textContent = result || 'Peer added';
            toast('Peer added by pubkey');
        } catch (e) { toast(e.message, true); }
    };

    window.vpnPeerRemove = async function () {
        const iface = document.getElementById('vpn-peer-remove-iface').value.trim();
        const public_key = document.getElementById('vpn-peer-remove-key').value.trim();
        if (!iface || !public_key) { toast('Interface and public key required', true); return; }
        try {
            await apiPostJson('/api/vpn/peer/remove', { interface: iface, public_key });
            toast('Peer removed');
        } catch (e) { toast(e.message, true); }
    };

    // ── Logs ───────────────────────────────────────────────────────────────
    async function loadLogSources() {
        try {
            const data = await apiGetJson('/api/logs/sources');
            const sel = document.getElementById('log-source');
            sel.innerHTML = '';
            data.forEach(s => {
                const opt = document.createElement('option');
                opt.value = s;
                opt.textContent = s;
                sel.appendChild(opt);
            });
        } catch (e) { /* ignore */ }
    }

    window.logsView = async function () {
        const source = document.getElementById('log-source').value;
        const lines = parseInt(document.getElementById('log-lines').value) || 50;
        const el = document.getElementById('logs-output');
        el.textContent = 'Loading...';
        try {
            const data = await apiGetJson('/api/logs/view?source=' + encodeURIComponent(source) + '&lines=' + lines);
            el.textContent = data.join('\n');
        } catch (e) { el.textContent = 'Error: ' + e.message; toast(e.message, true); }
    };

    window.logsInstall = async function () {
        try {
            const data = await apiGetJson('/api/logs/install');
            document.getElementById('logs-output').textContent = data.join('\n');
        } catch (e) { toast(e.message, true); }
    };

    window.logsSmokeTest = async function () {
        try {
            const data = await apiGetJson('/api/logs/smoke-test');
            document.getElementById('logs-output').textContent = data.join('\n');
        } catch (e) { toast(e.message, true); }
    };

    // ── Doctor ─────────────────────────────────────────────────────────────
    window.doctorRun = async function () {
        const el = document.getElementById('doctor-output');
        el.textContent = 'Running diagnostics...';
        try {
            const data = await apiGetJson('/api/doctor');
            el.textContent = data;
        } catch (e) { el.textContent = 'Error: ' + e.message; toast(e.message, true); }
    };

    window.doctorSecurity = async function () {
        const el = document.getElementById('doctor-output');
        el.textContent = 'Running security diagnostics...';
        try {
            const data = await apiGetJson('/api/doctor/security');
            el.textContent = data;
        } catch (e) { el.textContent = 'Error: ' + e.message; toast(e.message, true); }
    };

    // ── Maintenance ───────────────────────────────────────────────────────
    window.maintStatus = async function () {
        try { const data = await apiGetJson('/api/maintenance/status'); document.getElementById('maintenance-output').textContent = data; }
        catch (e) { toast(e.message, true); }
    };
    window.maintTimerStatus = async function () {
        try { const data = await apiGetJson('/api/maintenance/timer-status'); document.getElementById('maintenance-output').textContent = data; }
        catch (e) { toast(e.message, true); }
    };
    window.maintEnableChecks = async function () {
        try { await apiPostJson('/api/maintenance/enable-checks'); toast('Checks enabled'); }
        catch (e) { toast(e.message, true); }
    };
    window.maintDisableChecks = async function () {
        try { await apiPostJson('/api/maintenance/disable-checks'); toast('Checks disabled'); }
        catch (e) { toast(e.message, true); }
    };
    window.maintBackup = async function () {
        try { const result = await apiPostJson('/api/maintenance/backup'); document.getElementById('maintenance-output').textContent = result; toast('Backup done'); }
        catch (e) { toast(e.message, true); }
    };
    window.maintSmokeTest = async function () {
        try { const result = await apiPostJson('/api/maintenance/smoke-test'); document.getElementById('maintenance-output').textContent = result; }
        catch (e) { toast(e.message, true); }
    };
    window.maintSshConfig = async function () {
        try { const data = await apiGetJson('/api/maintenance/ssh-config'); document.getElementById('maintenance-output').textContent = JSON.stringify(data, null, 2); }
        catch (e) { toast(e.message, true); }
    };
    window.maintSshHardenPqc = async function () {
        const force = document.getElementById('maint-pqc-force').checked;
        const dry_run = document.getElementById('maint-pqc-dryrun').checked;
        try { const result = await apiPostJson('/api/maintenance/ssh-harden-pqc', { force, dry_run }); document.getElementById('maintenance-output').textContent = result; toast('SSH hardened'); }
        catch (e) { toast(e.message, true); }
    };
    window.maintCleanup = async function () {
        const target = document.getElementById('maint-cleanup-target').value;
        const keep_days = parseInt(document.getElementById('maint-cleanup-days').value) || 30;
        const dry_run = document.getElementById('maint-cleanup-dryrun').checked;
        try { const result = await apiPostJson('/api/maintenance/cleanup', { target, keep_days, dry_run }); document.getElementById('maintenance-output').textContent = JSON.stringify(result, null, 2); toast('Cleanup done'); }
        catch (e) { toast(e.message, true); }
    };

    // ── Diagnostics ────────────────────────────────────────────────────────
    async function diagFetch(url) {
        const el = document.getElementById('diag-output');
        el.textContent = 'Loading...';
        try { const data = await apiGetJson(url); el.textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2); }
        catch (e) { el.textContent = 'Error: ' + e.message; toast(e.message, true); }
    }
    window.diagSummary = () => diagFetch('/api/diag/summary');
    window.diagNginx = () => diagFetch('/api/diag/nginx');
    window.diagTor = () => diagFetch('/api/diag/tor');
    window.diagSsh = () => diagFetch('/api/diag/ssh');
    window.diagWordpress = () => diagFetch('/api/diag/wordpress');
    window.diagWpSync = () => diagFetch('/api/diag/wp-sync');
    window.diagNginxTest = () => diagFetch('/api/diag/nginx-test');
    window.diagResources = () => diagFetch('/api/diag/resources');

    // ── Test ───────────────────────────────────────────────────────────────
    window.testRun = async function () {
        const filter = document.getElementById('test-filter').value.trim() || null;
        const el = document.getElementById('test-output');
        el.textContent = 'Running tests...';
        try { const result = await apiPostJson('/api/test/run', { filter }); el.textContent = result; toast('Tests run'); }
        catch (e) { el.textContent = 'Error: ' + e.message; toast(e.message, true); }
    };
    window.testList = async function () {
        const el = document.getElementById('test-output');
        el.textContent = 'Loading...';
        try { const data = await apiGetJson('/api/test/list'); el.textContent = data.join('\n'); }
        catch (e) { el.textContent = 'Error: ' + e.message; toast(e.message, true); }
    };
    window.testBenchmark = async function () {
        try { const result = await apiPostJson('/api/test/benchmark'); document.getElementById('test-output').textContent = result; }
        catch (e) { toast(e.message, true); }
    };
    window.testResults = async function () {
        try { const data = await apiGetJson('/api/test/results'); document.getElementById('test-output').textContent = JSON.stringify(data, null, 2); }
        catch (e) { toast(e.message, true); }
    };
    window.testClean = async function () {
        try { const result = await apiPostJson('/api/test/clean'); document.getElementById('test-output').textContent = result; toast('Cleaned'); }
        catch (e) { toast(e.message, true); }
    };

    // ── Setup ──────────────────────────────────────────────────────────────
    window.setupRun = async function () {
        const all = document.getElementById('setup-all').checked;
        const vpn = document.getElementById('setup-vpn').checked;
        const security = document.getElementById('setup-security').checked;
        const pqc_tls = document.getElementById('setup-pqc-tls').checked;
        const out = document.getElementById('setup-output');

        if (pqc_tls) {
            out.textContent = 'Starting PQC TLS installer (streaming output)...\n';
            const evtSrc = new EventSource('/api/setup/pqc-tls');
            evtSrc.onmessage = function (ev) {
                out.textContent += ev.data + '\n';
                out.scrollTop = out.scrollHeight;
            };
            evtSrc.onerror = function () {
                evtSrc.close();
                if (!out.textContent.includes('SUCCESS') && !out.textContent.includes('FAILED') && !out.textContent.includes('TIMEOUT')) {
                    out.textContent += '\n[Connection closed unexpectedly]\n';
                    out.textContent += 'SOLUTION: Run "enola-cli setup --pqc-tls" directly in a terminal.\n';
                    toast('PQC TLS stream closed', true);
                } else {
                    toast('PQC TLS setup finished');
                }
            };
            return;
        }

        try { const result = await apiPostJson('/api/setup', { all, vpn, security, pqc_tls }); out.textContent = result || 'Setup complete'; toast('Setup complete'); }
        catch (e) { out.textContent = 'Error: ' + e.message; toast(e.message, true); }
    };

    // ── System (quickref, license, config, verify, uninstall) ─────────────
    window.sysQuickref = async function () {
        try { const data = await apiGetJson('/api/quickref'); document.getElementById('system-output').textContent = data; toast('Quick reference loaded'); }
        catch (e) { document.getElementById('system-output').textContent = 'Error: ' + e.message; toast(e.message, true); }
    };
    window.sysLicense = async function () {
        try { const data = await apiGetJson('/api/license'); document.getElementById('system-output').textContent = data; }
        catch (e) { document.getElementById('system-output').textContent = 'Error: ' + e.message; toast(e.message, true); }
    };
    window.sysConfigShow = async function () {
        try { const data = await apiGetJson('/api/config/show'); document.getElementById('system-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2); }
        catch (e) { document.getElementById('system-output').textContent = 'Error: ' + e.message; toast(e.message, true); }
    };
    window.sysConfigValidate = async function () {
        const reachable = document.getElementById('sys-config-reachable').checked;
        try { const data = await apiPostJson('/api/config/validate', { reachable }); document.getElementById('system-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2); toast('Config validated'); }
        catch (e) { document.getElementById('system-output').textContent = 'Error: ' + e.message; toast(e.message, true); }
    };
    window.sysVerify = async function () {
        const file = document.getElementById('sys-verify-file').value.trim();
        if (!file) { toast('File path required', true); return; }
        const pqsig = document.getElementById('sys-verify-pqsig').value.trim() || null;
        const pubkey = document.getElementById('sys-verify-pubkey').value.trim() || null;
        try {
            const data = await apiPostJson('/api/verify', { file, pqsig, pubkey });
            document.getElementById('system-output').textContent = JSON.stringify(data, null, 2);
            toast(data.verified ? 'Verified OK' : 'Verification failed', !data.verified);
        } catch (e) { toast(e.message, true); }
    };
    window.sysUninstall = async function () {
        showConfirm('Uninstall Enola CLI', 'Are you sure you want to uninstall Enola CLI? This is irreversible.', async () => {
            const yes = document.getElementById('sys-uninstall-yes').checked;
            const keep_data = document.getElementById('sys-uninstall-keepdata').checked;
            const remove_deps = document.getElementById('sys-uninstall-removedeps').checked;
            const force = document.getElementById('sys-uninstall-force').checked;
            const only = document.getElementById('sys-uninstall-only').value.trim() || null;
            try {
                const data = await apiPostJson('/api/uninstall', { yes, keep_data, remove_deps, force, only });
                document.getElementById('system-output').textContent = 'EXIT ' + data.exit_code + '\n=== STDOUT ===\n' + data.stdout + '\n=== STDERR ===\n' + data.stderr;
                toast('Uninstall completed');
            } catch (e) { toast(e.message, true); }
        });
    };

    // ── Update ─────────────────────────────────────────────────────────────
    window.updCheck = async function () {
        const force = document.getElementById('upd-check-force').checked;
        try {
            const data = await apiPostJson('/api/update/check', { force });
            document.getElementById('update-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
        } catch (e) { toast(e.message, true); }
    };
    window.updSchema = async function () {
        try {
            const data = await apiGetJson('/api/update/schema');
            document.getElementById('update-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
        } catch (e) { toast(e.message, true); }
    };
    window.updDownload = async function () {
        const yes = document.getElementById('upd-dl-yes').checked;
        const dry_run = document.getElementById('upd-dl-dryrun').checked;
        const force = document.getElementById('upd-dl-force').checked;
        const allow_unsigned = document.getElementById('upd-dl-allow-unsigned').checked;
        try {
            const data = await apiPostJson('/api/update/download', { yes, dry_run, force, allow_unsigned });
            document.getElementById('update-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
            toast('Download done');
        } catch (e) { toast(e.message, true); }
    };
    window.updApply = async function () {
        showConfirm('Apply Update', 'Apply update? This will replace the current binary.', async () => {
            const binary = document.getElementById('upd-apply-binary').value.trim() || null;
            const allow_unsigned = document.getElementById('upd-apply-allow-unsigned').checked;
            try {
                const data = await apiPostJson('/api/update/apply', { binary, allow_unsigned });
                document.getElementById('update-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
                toast('Update applied');
            } catch (e) { toast(e.message, true); }
        });
    };

    window.updVerifyFeed = async function () {
        const source = document.getElementById('upd-vf-source').value.trim();
        if (!source) { toast('Feed source required', true); return; }
        const signature = document.getElementById('upd-vf-signature').value.trim() || null;
        try {
            const data = await apiPostJson('/api/update/verify-feed', { source, signature });
            document.getElementById('update-output').textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
            toast('Feed verified');
        } catch (e) { toast(e.message, true); }
    };

    // ── Docs ───────────────────────────────────────────────────────────────
    window.docsShow = async function () {
        const topic = document.getElementById('docs-topic').value;
        const filter = document.getElementById('docs-filter').value.trim();
        let path = '/api/docs/' + encodeURIComponent(topic);
        if (topic === 'search' && filter) {
            path = '/api/docs/search/' + encodeURIComponent(filter);
        } else if (filter) {
            path += '/' + encodeURIComponent(filter);
        }
        try {
            const data = await apiGetJson(path);
            document.getElementById('docs-output').textContent = data;
        } catch (e) { toast(e.message, true); }
    };

    // ── Console ─────────────────────────────────────────────────────────────
    window.consolePreset = function () {
        const val = document.getElementById('console-preset').value;
        if (val) document.getElementById('console-args').value = val;
    };

    function parseArgs(raw) {
        const args = [];
        let current = '';
        let inQuotes = false;
        let quoteChar = '';
        for (let i = 0; i < raw.length; i++) {
            const ch = raw[i];
            if (inQuotes) {
                if (ch === quoteChar) {
                    inQuotes = false;
                } else {
                    current += ch;
                }
            } else if (ch === '"' || ch === "'") {
                inQuotes = true;
                quoteChar = ch;
            } else if (/\s/.test(ch)) {
                if (current) { args.push(current); current = ''; }
            } else {
                current += ch;
            }
        }
        if (current) args.push(current);
        return args;
    }

    let CONSOLE_DATA = { modules: [], subcommands: {}, flags: {} };

    async function loadConsoleCommands() {
        try {
            const res = await apiGet('/console_commands.json');
            if (res.ok) {
                CONSOLE_DATA = await res.json();
            }
        } catch(e) { /* fallback: empty data, console still works without suggestions */ }
    }

    function updateConsoleSuggestions() {
        const input = document.getElementById('console-args');
        const raw = input.value.trim();
        const parts = raw.split(/\s+/);
        const datalist = document.getElementById('console-suggestions');
        datalist.innerHTML = '';
        let suggestions = [];
        if (parts.length <= 1) {
            const prefix = parts[0] || '';
            suggestions = (CONSOLE_DATA.modules || []).filter(m => m.startsWith(prefix) && m !== prefix);
        } else if (parts.length === 2) {
            const mod = parts[0];
            const sub = parts[1];
            const subs = (CONSOLE_DATA.subcommands && CONSOLE_DATA.subcommands[mod]) || [];
            suggestions = subs.filter(s => s.startsWith(sub) && s !== sub).map(s => mod + ' ' + s);
        } else {
            const lastPart = parts[parts.length - 1];
            if (lastPart.startsWith('--')) {
                const mod = parts[0];
                const sub = parts[1];
                const flagKey = mod + '.' + sub;
                const flags = (CONSOLE_DATA.flags && (CONSOLE_DATA.flags[flagKey] || CONSOLE_DATA.flags[mod])) || [];
                const usedFlags = parts.slice(1, -1).filter(p => p.startsWith('--'));
                suggestions = flags
                    .filter(f => f.startsWith(lastPart) && f !== lastPart)
                    .filter(f => !usedFlags.includes(f))
                    .map(f => parts.slice(0, -1).join(' ') + ' ' + f);
            }
        }
        suggestions.slice(0, 10).forEach(s => {
            const opt = document.createElement('option');
            opt.value = s;
            datalist.appendChild(opt);
        });
    }

    let consoleHistory = [];
    let consoleHistoryIdx = -1;
    try { consoleHistory = JSON.parse(localStorage.getItem('enola_console_history') || '[]'); } catch(e) { consoleHistory = []; }

    function saveConsoleHistory(cmd) {
        if (cmd && cmd !== consoleHistory[consoleHistory.length - 1]) {
            consoleHistory.push(cmd);
            if (consoleHistory.length > 50) consoleHistory = consoleHistory.slice(-50);
            localStorage.setItem('enola_console_history', JSON.stringify(consoleHistory));
        }
        consoleHistoryIdx = consoleHistory.length;
    }

    document.addEventListener('DOMContentLoaded', function() {
        const input = document.getElementById('console-args');
        if (!input) return;
        input.addEventListener('input', updateConsoleSuggestions);
        input.addEventListener('keydown', function(e) {
            if (e.key === 'ArrowUp') {
                e.preventDefault();
                if (consoleHistoryIdx > 0) {
                    consoleHistoryIdx--;
                    input.value = consoleHistory[consoleHistoryIdx] || '';
                }
            } else if (e.key === 'ArrowDown') {
                e.preventDefault();
                if (consoleHistoryIdx < consoleHistory.length - 1) {
                    consoleHistoryIdx++;
                    input.value = consoleHistory[consoleHistoryIdx] || '';
                } else {
                    consoleHistoryIdx = consoleHistory.length;
                    input.value = '';
                }
            } else if (e.key === 'Enter') {
                consoleRun();
            }
        });
        consoleHistoryIdx = consoleHistory.length;
    });

    window.consoleRun = async function () {
        const raw = document.getElementById('console-args').value.trim();
        if (!raw) { toast('Type a command', true); return; }
        const args = parseArgs(raw);
        const out = document.getElementById('console-output');
        const mod = args[0];
        if (!(CONSOLE_DATA.modules || []).includes(mod)) {
            out.textContent = 'Error: Unknown module "' + mod + '". Valid: ' + (CONSOLE_DATA.modules || []).join(', ');
            toast('Unknown module: ' + mod, true);
            return;
        }
        saveConsoleHistory(raw);
        out.textContent = '$ ' + raw + '\nRunning...';
        try {
            const res = await apiPost('/api/console/run', { args, timeout_secs: 300 });
            const data = await res.json().catch(async () => ({ error: await res.text() }));
            if (!res.ok) {
                let detail = data.error || 'Command failed';
                if (data.stderr) detail += '\n=== STDERR ===\n' + data.stderr;
                if (data.exit_code !== undefined) detail += '\nEXIT ' + data.exit_code;
                out.textContent = '$ ' + raw + '\nError: ' + detail;
                toast(data.error || 'Command failed', true);
                return;
            }
            out.textContent = '$ ' + raw + '\nEXIT ' + data.exit_code + '\n=== STDOUT ===\n' + data.stdout + '\n=== STDERR ===\n' + data.stderr;
        } catch (e) {
            out.textContent = '$ ' + raw + '\nError: ' + e.message;
            toast(e.message, true);
        }
    };

})();
