/**
 * Mop Hello Plugin UI - Web Component Custom Element
 * Completely isolated from host internals (no import of Vue, Pinia, or Vue Router).
 */
class MopPluginHello extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' });
    this._context = null;
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
    const root = this.shadowRoot.querySelector('.hello-plugin-container');
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
        .hello-plugin-container {
          background: rgba(30, 41, 59, 0.7);
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 12px;
          padding: 24px;
          backdrop-filter: blur(12px);
          display: flex;
          flex-direction: column;
          gap: 20px;
        }
        .hello-plugin-container[data-theme="light"] {
          background: rgba(255, 255, 255, 0.85);
          color: #0f172a;
          border-color: rgba(0, 0, 0, 0.1);
        }
        .header {
          display: flex;
          align-items: center;
          gap: 12px;
        }
        .icon {
          font-size: 28px;
        }
        h2 {
          margin: 0;
          font-size: 1.25rem;
          font-weight: 600;
        }
        p {
          margin: 0;
          color: #94a3b8;
          font-size: 0.9rem;
        }
        .hello-plugin-container[data-theme="light"] p {
          color: #64748b;
        }
        .actions {
          display: flex;
          flex-wrap: wrap;
          gap: 12px;
        }
        button {
          background: linear-gradient(135deg, #3b82f6, #2563eb);
          color: white;
          border: none;
          border-radius: 8px;
          padding: 10px 18px;
          font-size: 0.9rem;
          font-weight: 500;
          cursor: pointer;
          transition: transform 0.15s ease, opacity 0.15s ease;
        }
        button:hover {
          opacity: 0.9;
          transform: translateY(-1px);
        }
        button:active {
          transform: translateY(0);
        }
        button.secondary {
          background: rgba(255, 255, 255, 0.1);
          color: inherit;
        }
        .hello-plugin-container[data-theme="light"] button.secondary {
          background: rgba(0, 0, 0, 0.08);
        }
        .output-box {
          background: rgba(15, 23, 42, 0.6);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 8px;
          padding: 16px;
          font-family: monospace;
          font-size: 0.85rem;
          min-height: 80px;
          white-space: pre-wrap;
          word-break: break-all;
          color: #38bdf8;
        }
        .hello-plugin-container[data-theme="light"] .output-box {
          background: #f8fafc;
          border-color: #e2e8f0;
          color: #0369a1;
        }
      </style>

      <div class="hello-plugin-container" data-theme="${theme}">
        <div class="header">
          <span class="icon">👋</span>
          <div>
            <h2>Hello Plugin</h2>
            <p>First-party Custom Element Plugin running inside sandbox</p>
          </div>
        </div>

        <div class="actions">
          <button id="btn-ping">Send Ping Job (hello.ping)</button>
          <button id="btn-doctor" class="secondary">Run Health Check</button>
          <button id="btn-describe" class="secondary">Inspect Plugin Meta</button>
        </div>

        <div>
          <p style="margin-bottom: 6px; font-weight: 500;">RPC Output Log:</p>
          <div id="output" class="output-box">Ready. Click a button to test JSON-RPC communication with plugin backend.</div>
        </div>
      </div>
    `;

    this.shadowRoot.getElementById('btn-ping')?.addEventListener('click', () => this.sendPing());
    this.shadowRoot.getElementById('btn-doctor')?.addEventListener('click', () => this.runDoctor());
    this.shadowRoot.getElementById('btn-describe')?.addEventListener('click', () => this.inspectMeta());
  }

  async sendPing() {
    const output = this.shadowRoot.getElementById('output');
    if (!this._context || !this._context.callRpc) {
      if (output) output.textContent = 'Error: PluginContext or callRpc not provided by host';
      return;
    }

    try {
      if (output) output.textContent = 'Submitting hello.ping job...';
      const res = await this._context.callRpc('job.submit', {
        job_type: 'hello.ping',
        payload: { message: 'Ping from UI' }
      });
      if (output) output.textContent = 'Job submitted successfully:\n' + JSON.stringify(res, null, 2);
    } catch (e) {
      if (output) output.textContent = 'Failed to submit job:\n' + (e.message || String(e));
    }
  }

  async runDoctor() {
    const output = this.shadowRoot.getElementById('output');
    if (!this._context || !this._context.callRpc) {
      if (output) output.textContent = 'Error: PluginContext not available';
      return;
    }

    try {
      if (output) output.textContent = 'Running doctor diagnosis...';
      const res = await this._context.callRpc('doctor', null);
      if (output) output.textContent = 'Doctor response:\n' + JSON.stringify(res, null, 2);
    } catch (e) {
      if (output) output.textContent = 'Doctor error:\n' + (e.message || String(e));
    }
  }

  async inspectMeta() {
    const output = this.shadowRoot.getElementById('output');
    if (!this._context || !this._context.callRpc) {
      if (output) output.textContent = 'Error: PluginContext not available';
      return;
    }

    try {
      if (output) output.textContent = 'Querying describe...';
      const res = await this._context.callRpc('describe', null);
      if (output) output.textContent = 'Describe response:\n' + JSON.stringify(res, null, 2);
    } catch (e) {
      if (output) output.textContent = 'Describe error:\n' + (e.message || String(e));
    }
  }
}

if (!customElements.get('mop-plugin-hello')) {
  customElements.define('mop-plugin-hello', MopPluginHello);
}

export default MopPluginHello;
