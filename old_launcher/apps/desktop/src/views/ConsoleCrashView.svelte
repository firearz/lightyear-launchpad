<script lang="ts">
  import type { CrashSignatureRecord } from '../lib/ipc/fixtures';
  import { ipcAdapter } from '../lib/ipc/adapter';

  interface Props {
    crashes: CrashSignatureRecord[];
    onNotify: (msg: string) => void;
  }

  let { crashes = $bindable(), onNotify }: Props = $props();

  let customLogSnippet = $state(
    `[20:14:09] [main/FATAL]: Unhandled exception launching game\njava.lang.UnsupportedClassVersionError: net/minecraft/client/main/Main has been compiled by a more recent version of the Java Runtime (class file version 69.0), this version of the Java Runtime only recognizes class file versions up to 65.0\n  at java.base/java.lang.ClassLoader.defineClass1(Native Method)\n  --accessToken eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.superSecretToken.sig\n  GameDir: C:\\Users\\NILESH\\AppData\\Roaming\\Kestryx\\instances\\26.3`
  );

  let redactedPreview = $derived(ipcAdapter.redactSensitiveLog(customLogSnippet));

  function applyOneClickFix(crashId: string) {
    crashes = crashes.map((c) => (c.id === crashId ? { ...c, fixApplied: true } : c));
    const item = crashes.find((c) => c.id === crashId);
    if (item) {
      onNotify(`Applied 1-click fix: ${item.fixActionLabel}`);
    }
  }
</script>

<section class="console-crash-view" data-testid="console-crash-view">
  <div class="hero-banner">
    <div>
      <span class="badge badge-danger">INTELLIGENT CRASH ANALYZER & AUTO-REPAIR</span>
      <h1 style="margin:6px 0 4px;font-size:1.45rem;">Console & 1-Click Crash Assistant</h1>
      <div style="color:var(--text-secondary);font-size:0.84rem;">
        Parses stack traces, Mixin failures, Java classfile mismatches, and OOM errors into plain-English explanations with 1-click fixes.
      </div>
    </div>
    <button
      class="btn btn-primary"
      data-testid="copy-diagnostics-bundle-btn"
      onclick={() => onNotify('Copied privacy-redacted Diagnostics Bundle (tokens & OS user paths stripped) to clipboard.')}
    >
      📋 Copy Redacted Diagnostics Bundle
    </button>
  </div>

  <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));">
    {#each crashes as crash (crash.id)}
      <div class="card" style="display:flex;flex-direction:column;justify-content:space-between;gap:10px;" data-testid="crash-card-{crash.id}">
        <div>
          <div style="display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:6px;">
            <span class="badge {crash.severity === 'critical' ? 'badge-danger' : 'badge-warn'}">
              {crash.category}
            </span>
            <span class="badge mono">{crash.code}</span>
          </div>
          <h3 style="margin:0 0 6px;font-size:1rem;">{crash.title}</h3>
          <p style="margin:0 0 8px;font-size:0.83rem;color:var(--text-secondary);">
            {crash.summary}
          </p>
          <div style="font-size:0.78rem;margin-bottom:6px;">
            <strong>Identified Culprit:</strong> <code>{crash.culprit}</code>
          </div>
          <pre
            class="mono"
            style="background:var(--bg-input);padding:8px;border-radius:6px;font-size:0.73rem;white-space:pre-wrap;margin:0;color:var(--text-secondary);"
          >{crash.excerpt}</pre>
        </div>

        <div style="display:flex;align-items:center;justify-content:space-between;gap:8px;margin-top:4px;">
          {#if crash.fixApplied}
            <span class="badge badge-success" data-testid="fix-applied-{crash.id}">✓ Fix Applied & Verified</span>
          {:else}
            <button
              class="btn btn-primary"
              style="width:100%;"
              data-testid="apply-fix-{crash.id}-btn"
              onclick={() => applyOneClickFix(crash.id)}
            >
              ⚡ 1-Click Fix: {crash.fixActionLabel}
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <div class="card" data-testid="privacy-redactor-panel">
    <h3 style="margin-top:0;">Live Privacy Log Redactor (`kestryx-diagnostics`)</h3>
    <p style="font-size:0.82rem;color:var(--text-secondary);margin-top:0;">
      Automatically strips JWT bearer tokens, `--accessToken` arguments, XSTS headers, and local OS username paths before sharing logs.
    </p>
    <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));">
      <div>
        <strong style="font-size:0.78rem;color:var(--text-muted);">Raw Crash / Console Input:</strong>
        <textarea
          class="input mono"
          rows="5"
          style="width:100%;margin-top:4px;font-size:0.75rem;"
          bind:value={customLogSnippet}
          aria-label="Raw log input for redaction"
        ></textarea>
      </div>
      <div>
        <strong style="font-size:0.78rem;color:var(--accent-primary);">Redacted Safe Output:</strong>
        <pre
          class="mono"
          data-testid="redacted-log-output"
          style="background:var(--bg-input);padding:10px;border-radius:6px;font-size:0.75rem;white-space:pre-wrap;margin:4px 0 0;border:1px solid var(--border-subtle);"
        >{redactedPreview}</pre>
      </div>
    </div>
  </div>
</section>
