/**
 * Mop Video Plugin UI - Web Component Custom Element (<mop-plugin-video>)
 * Isolated from host internals (no import of Vue, Pinia, or Vue Router).
 * Context: { pluginId, apiBaseUrl, currentUser, theme, callRpc, showNotification }
 */
class MopPluginVideo extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' });
    this._context = null;
    this._activeTab = 'convert';
    this._themeHandler = this.onThemeChange.bind(this);
  }

  get context() {
    return this._context;
  }

  set context(val) {
    this._context = val;
    this.render();
  }

  connectedCallback() {
    window.addEventListener('mop:theme', this._themeHandler);
    this.render();
  }

  disconnectedCallback() {
    window.removeEventListener('mop:theme', this._themeHandler);
  }

  onThemeChange(e) {
    if (this._context && e.detail && e.detail.theme) {
      this._context.theme = e.detail.theme;
      this.updateTheme();
    }
  }

  updateTheme() {
    const root = this.shadowRoot.querySelector('.video-plugin-container');
    if (root && this._context) {
      root.setAttribute('data-theme', this._context.theme || 'dark');
    }
  }

  render() {
    if (!this.shadowRoot) return;

    const theme = (this._context && this._context.theme) || 'dark';

    this.shadowRoot.innerHTML = `
      <style>
        :host {
          display: block;
          font-family: inherit;
          color: var(--text-color, #f1f5f9);
        }
        .video-plugin-container {
          background: rgba(30, 41, 59, 0.7);
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 12px;
          padding: 24px;
          backdrop-filter: blur(12px);
          display: flex;
          flex-direction: column;
          gap: 20px;
        }
        .video-plugin-container[data-theme="light"] {
          background: rgba(255, 255, 255, 0.9);
          color: #0f172a;
          border-color: rgba(0, 0, 0, 0.1);
        }
        .header {
          display: flex;
          align-items: center;
          justify-content: space-between;
          border-bottom: 1px solid rgba(255, 255, 255, 0.08);
          padding-bottom: 16px;
        }
        .video-plugin-container[data-theme="light"] .header {
          border-bottom-color: rgba(0, 0, 0, 0.08);
        }
        .title-group {
          display: flex;
          align-items: center;
          gap: 12px;
        }
        .icon {
          font-size: 30px;
        }
        h2 {
          margin: 0;
          font-size: 1.25rem;
          font-weight: 600;
        }
        p {
          margin: 0;
          color: #94a3b8;
          font-size: 0.88rem;
        }
        .video-plugin-container[data-theme="light"] p {
          color: #64748b;
        }
        .tab-bar {
          display: flex;
          gap: 8px;
          border-bottom: 1px solid rgba(255, 255, 255, 0.08);
          padding-bottom: 8px;
          overflow-x: auto;
        }
        .video-plugin-container[data-theme="light"] .tab-bar {
          border-bottom-color: rgba(0, 0, 0, 0.08);
        }
        .tab-btn {
          background: transparent;
          border: none;
          color: #94a3b8;
          padding: 8px 14px;
          border-radius: 6px;
          font-size: 0.9rem;
          font-weight: 500;
          cursor: pointer;
          transition: all 0.15s ease;
        }
        .video-plugin-container[data-theme="light"] .tab-btn {
          color: #64748b;
        }
        .tab-btn:hover {
          color: #f1f5f9;
          background: rgba(255, 255, 255, 0.05);
        }
        .video-plugin-container[data-theme="light"] .tab-btn:hover {
          color: #0f172a;
          background: rgba(0, 0, 0, 0.05);
        }
        .tab-btn.active {
          color: #ffffff;
          background: #8b5cf6;
        }
        .video-plugin-container[data-theme="light"] .tab-btn.active {
          color: #ffffff;
          background: #7c3aed;
        }
        .tab-content {
          display: none;
          flex-direction: column;
          gap: 16px;
        }
        .tab-content.active {
          display: flex;
        }
        .form-group {
          display: flex;
          flex-direction: column;
          gap: 6px;
        }
        label {
          font-size: 0.85rem;
          font-weight: 500;
        }
        input[type="text"], input[type="password"] {
          background: rgba(15, 23, 42, 0.6);
          border: 1px solid rgba(255, 255, 255, 0.12);
          border-radius: 6px;
          padding: 8px 12px;
          color: inherit;
          font-size: 0.9rem;
        }
        .video-plugin-container[data-theme="light"] input[type="text"],
        .video-plugin-container[data-theme="light"] input[type="password"] {
          background: #ffffff;
          border-color: #cbd5e1;
          color: #0f172a;
        }
        .checkbox-group {
          display: flex;
          align-items: center;
          gap: 8px;
          font-size: 0.88rem;
        }
        .actions-row {
          display: flex;
          gap: 12px;
        }
        button.btn-primary {
          background: linear-gradient(135deg, #8b5cf6, #7c3aed);
          color: white;
          border: none;
          border-radius: 8px;
          padding: 10px 18px;
          font-size: 0.9rem;
          font-weight: 500;
          cursor: pointer;
          transition: opacity 0.15s ease, transform 0.15s ease;
        }
        button.btn-primary:hover {
          opacity: 0.9;
          transform: translateY(-1px);
        }
        button.btn-secondary {
          background: rgba(255, 255, 255, 0.1);
          color: inherit;
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 8px;
          padding: 10px 18px;
          font-size: 0.9rem;
          font-weight: 500;
          cursor: pointer;
          transition: background 0.15s ease;
        }
        .video-plugin-container[data-theme="light"] button.btn-secondary {
          background: #e2e8f0;
          border-color: #cbd5e1;
          color: #1e293b;
        }
        .output-box {
          background: rgba(15, 23, 42, 0.7);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 8px;
          padding: 14px;
          font-family: monospace;
          font-size: 0.85rem;
          min-height: 90px;
          white-space: pre-wrap;
          word-break: break-all;
          color: #a78bfa;
        }
        .video-plugin-container[data-theme="light"] .output-box {
          background: #f8fafc;
          border-color: #e2e8f0;
          color: #6d28d9;
        }
        .doctor-card {
          padding: 14px;
          border-radius: 8px;
          background: rgba(255, 255, 255, 0.03);
          border: 1px solid rgba(255, 255, 255, 0.06);
          display: flex;
          flex-direction: column;
          gap: 8px;
        }
        .badge {
          display: inline-block;
          padding: 2px 8px;
          border-radius: 4px;
          font-size: 0.75rem;
          font-weight: 600;
        }
        .badge-ok {
          background: rgba(34, 197, 94, 0.2);
          color: #4ade80;
        }
      </style>

      <div class="video-plugin-container" data-theme="${theme}">
        <div class="header">
          <div class="title-group">
            <span class="icon">🎬</span>
            <div>
              <h2>Video Plugin (mop.video)</h2>
              <p>HEVC MP4 Transcoder & Directory Watcher</p>
            </div>
          </div>
          <div>
            <button id="btn-run-doctor-header" class="btn-secondary">🩺 FFmpeg 診断</button>
          </div>
        </div>

        <div class="tab-bar">
          <button class="tab-btn active" data-tab="convert" id="tab-convert">単一変換 (video.convert)</button>
          <button class="tab-btn" data-tab="batch" id="tab-batch">一括変換 (video.batch)</button>
          <button class="tab-btn" data-tab="inspect" id="tab-inspect">動画検査 (video.inspect)</button>
          <button class="tab-btn" data-tab="doctor" id="tab-doctor">FFmpeg 診断 (doctor)</button>
          <button class="tab-btn" data-tab="watcher" id="tab-watcher">監視・設定</button>
        </div>

        <!-- Tab 1: video.convert -->
        <div class="tab-content active" id="pane-convert">
          <div class="form-group">
            <label for="convert-input">入力ファイル / アーカイブパス (MKV / MP4 / AVI / ZIP 等):</label>
            <input type="text" id="convert-input" placeholder="/srv/incoming/video.mkv" />
          </div>
          <div class="form-group">
            <label for="convert-output">出力 MP4 パス (任意、省略時は video_dir に配置):</label>
            <input type="text" id="convert-output" placeholder="/srv/videos/output.mp4" />
          </div>
          <div class="form-group">
            <label for="convert-password">アーカイブパスワード (アーカイブ入力時のみ):</label>
            <input type="password" id="convert-password" placeholder="パスワード" />
          </div>
          <div class="checkbox-group">
            <input type="checkbox" id="convert-dry-run" />
            <label for="convert-dry-run">Dry Run (トランスコードを行わずファイル確認のみ実施)</label>
          </div>
          <div class="actions-row">
            <button id="btn-convert-submit" class="btn-primary">トランスコード実行</button>
          </div>
        </div>

        <!-- Tab 2: video.batch -->
        <div class="tab-content" id="pane-batch">
          <div class="form-group">
            <label for="batch-input">対象ディレクトリパス:</label>
            <input type="text" id="batch-input" placeholder="/srv/incoming" />
          </div>
          <div class="form-group">
            <label for="batch-password">アーカイブパスワード (任意):</label>
            <input type="password" id="batch-password" placeholder="パスワード" />
          </div>
          <div class="checkbox-group">
            <input type="checkbox" id="batch-dry-run" />
            <label for="batch-dry-run">Dry Run</label>
          </div>
          <div class="actions-row">
            <button id="btn-batch-submit" class="btn-primary">一括変換ジョブ実行</button>
          </div>
        </div>

        <!-- Tab 3: video.inspect -->
        <div class="tab-content" id="pane-inspect">
          <div class="form-group">
            <label for="inspect-input">検査動画 / アーカイブパス:</label>
            <input type="text" id="inspect-input" placeholder="/srv/incoming/video.mkv" />
          </div>
          <div class="form-group">
            <label for="inspect-password">パスワード (任意):</label>
            <input type="password" id="inspect-password" placeholder="パスワード" />
          </div>
          <div class="actions-row">
            <button id="btn-inspect-submit" class="btn-primary">検査実行</button>
          </div>
        </div>

        <!-- Tab 4: doctor -->
        <div class="tab-content" id="pane-doctor">
          <p>FFmpeg バイナリ、libx265 (HEVC) エンコーダ対応、およびディレクトリレイアウトを診断します。</p>
          <div class="actions-row">
            <button id="btn-run-doctor" class="btn-primary">FFmpeg 診断を実行</button>
          </div>
          <div id="doctor-results" class="doctor-card" style="display: none;">
            <div id="doctor-status-line"></div>
            <div id="doctor-checks-list"></div>
          </div>
        </div>

        <!-- Tab 5: watcher & config -->
        <div class="tab-content" id="pane-watcher">
          <p>常駐ウォッチャおよび動画変換設定を照会します。</p>
          <div class="actions-row">
            <button id="btn-inspect-meta" class="btn-primary">メタデータ (describe) を照会</button>
            <button id="btn-fetch-schema" class="btn-secondary">設定スキーマを照会</button>
          </div>
        </div>

        <div>
          <p style="margin-bottom: 6px; font-weight: 500;">実行結果ログ / RPC レスポンス:</p>
          <div id="output" class="output-box">準備完了。操作を選択してください。</div>
        </div>
      </div>
    `;

    this.bindEvents();
  }

  bindEvents() {
    // Tabs switching
    this.shadowRoot.querySelectorAll('.tab-btn').forEach((btn) => {
      btn.addEventListener('click', () => {
        const tab = btn.getAttribute('data-tab');
        this.switchTab(tab);
      });
    });

    // Header doctor button
    this.shadowRoot.getElementById('btn-run-doctor-header')?.addEventListener('click', () => {
      this.switchTab('doctor');
      this.runDoctor();
    });

    // Convert job
    this.shadowRoot.getElementById('btn-convert-submit')?.addEventListener('click', () => this.submitConvertJob());

    // Batch job
    this.shadowRoot.getElementById('btn-batch-submit')?.addEventListener('click', () => this.submitBatchJob());

    // Inspect job
    this.shadowRoot.getElementById('btn-inspect-submit')?.addEventListener('click', () => this.submitInspectJob());

    // Doctor
    this.shadowRoot.getElementById('btn-run-doctor')?.addEventListener('click', () => this.runDoctor());

    // Meta & Schema
    this.shadowRoot.getElementById('btn-inspect-meta')?.addEventListener('click', () => this.inspectMeta());
    this.shadowRoot.getElementById('btn-fetch-schema')?.addEventListener('click', () => this.fetchSchema());
  }

  switchTab(tabName) {
    this._activeTab = tabName;
    this.shadowRoot.querySelectorAll('.tab-btn').forEach((btn) => {
      btn.classList.toggle('active', btn.getAttribute('data-tab') === tabName);
    });
    this.shadowRoot.querySelectorAll('.tab-content').forEach((pane) => {
      pane.classList.toggle('active', pane.id === `pane-${tabName}`);
    });
  }

  setOutput(text) {
    const el = this.shadowRoot.getElementById('output');
    if (el) el.textContent = text;
  }

  async submitConvertJob() {
    if (!this._context || !this._context.callRpc) {
      this.setOutput('Error: PluginContext or callRpc not available');
      return;
    }
    const input = this.shadowRoot.getElementById('convert-input')?.value?.trim();
    if (!input) {
      this.setOutput('エラー: 入力ファイルパスを指定してください');
      return;
    }
    const output = this.shadowRoot.getElementById('convert-output')?.value?.trim() || null;
    const password = this.shadowRoot.getElementById('convert-password')?.value || null;
    const dryRun = !!this.shadowRoot.getElementById('convert-dry-run')?.checked;

    try {
      this.setOutput('video.convert ジョブを送信中...');
      const res = await this._context.callRpc('job.submit', {
        job_type: 'video.convert',
        params: { input, output, password, dry_run: dryRun },
      });
      this.setOutput('ジョブ送信完了:\n' + JSON.stringify(res, null, 2));
      if (this._context.showNotification) {
        this._context.showNotification('success', 'video.convert ジョブを送信しました');
      }
    } catch (e) {
      this.setOutput('ジョブ送信エラー:\n' + (e.message || String(e)));
    }
  }

  async submitBatchJob() {
    if (!this._context || !this._context.callRpc) {
      this.setOutput('Error: PluginContext or callRpc not available');
      return;
    }
    const dir = this.shadowRoot.getElementById('batch-input')?.value?.trim();
    if (!dir) {
      this.setOutput('エラー: 対象ディレクトリパスを指定してください');
      return;
    }
    const password = this.shadowRoot.getElementById('batch-password')?.value || null;
    const dryRun = !!this.shadowRoot.getElementById('batch-dry-run')?.checked;

    try {
      this.setOutput('video.batch ジョブを送信中...');
      const res = await this._context.callRpc('job.submit', {
        job_type: 'video.batch',
        params: { dir, password, dry_run: dryRun },
      });
      this.setOutput('ジョブ送信完了:\n' + JSON.stringify(res, null, 2));
      if (this._context.showNotification) {
        this._context.showNotification('success', 'video.batch ジョブを送信しました');
      }
    } catch (e) {
      this.setOutput('ジョブ送信エラー:\n' + (e.message || String(e)));
    }
  }

  async submitInspectJob() {
    if (!this._context || !this._context.callRpc) {
      this.setOutput('Error: PluginContext or callRpc not available');
      return;
    }
    const input = this.shadowRoot.getElementById('inspect-input')?.value?.trim();
    if (!input) {
      this.setOutput('エラー: 検査対象パスを指定してください');
      return;
    }
    const password = this.shadowRoot.getElementById('inspect-password')?.value || null;

    try {
      this.setOutput('video.inspect ジョブを送信中...');
      const res = await this._context.callRpc('job.submit', {
        job_type: 'video.inspect',
        params: { input, password },
      });
      this.setOutput('ジョブ送信完了:\n' + JSON.stringify(res, null, 2));
      if (this._context.showNotification) {
        this._context.showNotification('success', 'video.inspect ジョブを送信しました');
      }
    } catch (e) {
      this.setOutput('ジョブ送信エラー:\n' + (e.message || String(e)));
    }
  }

  async runDoctor() {
    if (!this._context || !this._context.callRpc) {
      this.setOutput('Error: PluginContext or callRpc not available');
      return;
    }

    try {
      this.setOutput('FFmpeg 診断を実行中...');
      const res = await this._context.callRpc('doctor', null);
      this.setOutput('FFmpeg 診断結果:\n' + JSON.stringify(res, null, 2));

      const card = this.shadowRoot.getElementById('doctor-results');
      const statusLine = this.shadowRoot.getElementById('doctor-status-line');
      const checksList = this.shadowRoot.getElementById('doctor-checks-list');

      if (card && statusLine && checksList && res) {
        card.style.display = 'flex';
        statusLine.innerHTML = `<strong>総合ステータス:</strong> <span class="badge badge-ok">${res.status || 'ok'}</span>`;
        if (Array.isArray(res.checks)) {
          checksList.innerHTML = res.checks
            .map(
              (c) =>
                `<div style="padding: 4px 0;">• <strong>${c.name}</strong> [${c.status}]: ${c.message || ''}</div>`
            )
            .join('');
        }
      }
    } catch (e) {
      this.setOutput('FFmpeg 診断エラー:\n' + (e.message || String(e)));
    }
  }

  async inspectMeta() {
    if (!this._context || !this._context.callRpc) {
      this.setOutput('Error: PluginContext not available');
      return;
    }
    try {
      this.setOutput('describe RPC を呼び出し中...');
      const res = await this._context.callRpc('describe', null);
      this.setOutput('describe レスポンス:\n' + JSON.stringify(res, null, 2));
    } catch (e) {
      this.setOutput('describe エラー:\n' + (e.message || String(e)));
    }
  }

  async fetchSchema() {
    if (!this._context || !this._context.callRpc) {
      this.setOutput('Error: PluginContext not available');
      return;
    }
    try {
      this.setOutput('config.schema RPC を呼び出し中...');
      const res = await this._context.callRpc('config.schema', null);
      this.setOutput('config.schema レスポンス:\n' + JSON.stringify(res, null, 2));
    } catch (e) {
      this.setOutput('config.schema エラー:\n' + (e.message || String(e)));
    }
  }
}

if (!customElements.get('mop-plugin-video')) {
  customElements.define('mop-plugin-video', MopPluginVideo);
}

export default MopPluginVideo;
