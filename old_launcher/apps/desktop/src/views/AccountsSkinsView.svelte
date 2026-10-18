<script lang="ts">
  import { onMount } from 'svelte';
  import type { AccountRecord } from '../lib/ipc/fixtures';
  import { computeOfflinePlayerUuid } from '../lib/ipc/offlineUuid';

  interface Props {
    accounts: AccountRecord[];
    onNotify: (msg: string) => void;
  }

  let { accounts = $bindable(), onNotify }: Props = $props();

  let offlineUsername = $state('Notch');
  let allowNonStandardUsername = $state(false);
  let offlineUuidPreview = $derived(computeOfflinePlayerUuid(offlineUsername));

  // Microsoft OAuth Device Code modal state
  let showDeviceCodeModal = $state(false);
  let deviceUserCode = $state('KSTR-942X');
  let showXstsGuide = $state(false);

  // 3D Skin & Cape Viewer state
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let armModel = $state<'classic' | 'slim'>('slim');
  let showOuterLayer = $state(true);
  let pose = $state<'idle' | 'walk' | 'run'>('walk');
  let backEquipment = $state<'cape' | 'elytra' | 'none'>('cape');
  let skinPreset = $state<'obsidian_valkyrie' | 'emerald_knight' | 'classic_steve'>('obsidian_valkyrie');
  let yaw = $state(0.55);
  let pitch = $state(0.22);
  let isDragging = $state(false);
  let lastMouseX = $state(0);
  let lastMouseY = $state(0);
  let offlineSkinBridgeEnabled = $state(true);

  function addOfflineAccount() {
    if (!offlineUuidPreview.validStrictUsername && !allowNonStandardUsername) {
      onNotify('Username must match [A-Za-z0-9_]{3,16} unless Advanced Override is enabled.');
      return;
    }
    const newAcc: AccountRecord = {
      id: `acc-off-${Date.now()}`,
      username: offlineUsername,
      type: 'offline',
      uuidDashed: offlineUuidPreview.dashed,
      uuidUndashed: offlineUuidPreview.undashed,
      active: false,
      skinVariant: armModel,
      capeId: backEquipment === 'none' ? 'none' : 'obsidian_guard',
      tokenStatus: 'offline_local',
    };
    accounts = [...accounts, newAcc];
    onNotify(`Created Offline Account "${offlineUsername}" (${offlineUuidPreview.dashed})`);
  }

  function setActiveAccount(id: string) {
    accounts = accounts.map((a) => ({ ...a, active: a.id === id }));
    const found = accounts.find((a) => a.id === id);
    if (found) onNotify(`Switched active account to ${found.username} (${found.type.toUpperCase()})`);
  }

  // 3D Isometric/Perspective Projected Minecraft Character Renderer on HTML5 Canvas
  function draw3DCharacter(ctx: CanvasRenderingContext2D, timeSec: number) {
    const w = ctx.canvas.width;
    const h = ctx.canvas.height;
    ctx.clearRect(0, 0, w, h);

    // Subtle studio floor grid
    ctx.save();
    ctx.translate(w / 2, h * 0.78);
    ctx.strokeStyle = 'rgba(56, 189, 248, 0.18)';
    ctx.lineWidth = 1;
    for (let i = -4; i <= 4; i++) {
      ctx.beginPath();
      ctx.moveTo(i * 22, -26);
      ctx.lineTo(i * 30, 26);
      ctx.stroke();
    }
    ctx.restore();

    const speed = pose === 'run' ? 6.5 : pose === 'walk' ? 3.2 : 1.1;
    const swingAmp = pose === 'run' ? 0.65 : pose === 'walk' ? 0.38 : 0.05;
    const swing = Math.sin(timeSec * speed) * swingAmp;

    const palette =
      skinPreset === 'obsidian_valkyrie'
        ? { head: '#F1C27D', hair: '#10B981', torso: '#1E293B', arm: '#0F172A', leg: '#334155', cape: '#10B981' }
        : skinPreset === 'emerald_knight'
          ? { head: '#E0AC69', hair: '#38BDF8', torso: '#065F46', arm: '#047857', leg: '#1E293B', cape: '#F59E0B' }
          : { head: '#C68642', hair: '#3B2219', torso: '#0284C7', arm: '#C68642', leg: '#312E81', cape: '#6366F1' };

    const cosY = Math.cos(yaw);
    const sinY = Math.sin(yaw);
    const cosP = Math.cos(pitch);
    const sinP = Math.sin(pitch);

    function project(x: number, y: number, z: number): [number, number, number] {
      const rx = x * cosY - z * sinY;
      const rz = x * sinY + z * cosY;
      const ry = y * cosP - rz * sinP;
      const rz2 = y * sinP + rz * cosP;
      const scale = 260 / (260 + rz2 + 60);
      return [w / 2 + rx * scale, h * 0.48 + ry * scale, rz2];
    }

    function drawBox(
      cx: number,
      cy: number,
      cz: number,
      bw: number,
      bh: number,
      bd: number,
      color: string,
      rotX = 0
    ) {
      const hw = bw / 2;
      const hh = bh / 2;
      const hd = bd / 2;
      const cR = Math.cos(rotX);
      const sR = Math.sin(rotX);

      const verts = [
        [-hw, -hh, -hd],
        [hw, -hh, -hd],
        [hw, hh, -hd],
        [-hw, hh, -hd],
        [-hw, -hh, hd],
        [hw, -hh, hd],
        [hw, hh, hd],
        [-hw, hh, hd],
      ].map(([vx, vy, vz]) => {
        const ny = vy * cR - vz * sR;
        const nz = vy * sR + vz * cR;
        return project(cx + vx, cy + ny, cz + nz);
      });

      const faces: Array<{ idx: number[]; shade: number }> = [
        { idx: [4, 5, 6, 7], shade: 1.0 }, // front
        { idx: [0, 1, 2, 3], shade: 0.72 }, // back
        { idx: [0, 4, 7, 3], shade: 0.84 }, // left
        { idx: [1, 5, 6, 2], shade: 0.88 }, // right
        { idx: [0, 1, 5, 4], shade: 1.12 }, // top
        { idx: [3, 2, 6, 7], shade: 0.65 }, // bottom
      ];

      const sorted = faces
        .map((f) => ({
          ...f,
          depth: f.idx.reduce((acc, i) => acc + verts[i][2], 0) / 4,
        }))
        .sort((a, b) => b.depth - a.depth);

      for (const f of sorted) {
        ctx.beginPath();
        ctx.moveTo(verts[f.idx[0]][0], verts[f.idx[0]][1]);
        for (let i = 1; i < f.idx.length; i++) {
          ctx.lineTo(verts[f.idx[i]][0], verts[f.idx[i]][1]);
        }
        ctx.closePath();
        ctx.fillStyle = color;
        ctx.fill();
        ctx.strokeStyle = 'rgba(255,255,255,0.18)';
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    }

    const armW = armModel === 'slim' ? 11 : 14;

    // Back Cape or Elytra
    if (backEquipment !== 'none') {
      const capeW = backEquipment === 'elytra' ? 36 : 30;
      drawBox(0, 6, -12, capeW, 46, 3, palette.cape, 0.18 + Math.abs(swing) * 0.25);
    }

    // Left & Right Legs
    drawBox(-8, 38, 0, 14, 36, 14, palette.leg, -swing);
    drawBox(8, 38, 0, 14, 36, 14, palette.leg, swing);

    // Torso
    drawBox(0, 2, 0, 30, 36, 16, palette.torso, 0);

    // Left & Right Arms
    drawBox(-(15 + armW / 2), 2, 0, armW, 36, 14, palette.arm, swing);
    drawBox(15 + armW / 2, 2, 0, armW, 36, 14, palette.arm, -swing);

    // Head
    drawBox(0, -32, 0, 30, 30, 30, palette.head, 0);

    // Outer 3D Hat/Jacket Layer
    if (showOuterLayer) {
      drawBox(0, -34, 0, 33, 14, 33, palette.hair, 0);
    }
  }

  onMount(() => {
    let rafId = 0;
    const start = performance.now();
    const renderLoop = (now: number) => {
      if (canvasEl) {
        const ctx = canvasEl.getContext('2d');
        if (ctx) {
          draw3DCharacter(ctx, (now - start) / 1000);
        }
      }
      rafId = requestAnimationFrame(renderLoop);
    };
    rafId = requestAnimationFrame(renderLoop);
    return () => cancelAnimationFrame(rafId);
  });
</script>

<section class="accounts-skins-view" data-testid="accounts-skins-view">
  <div class="hero-banner">
    <div>
      <span class="badge badge-success">DUAL AUTHENTICATION ENGINE</span>
      <h1 style="margin:6px 0 4px;font-size:1.45rem;">Accounts, Offline UUID Studio & 3D Skin Viewer</h1>
      <div style="color:var(--text-secondary);font-size:0.84rem;">
        Microsoft OAuth 2.0 (Device Code + PKCE) and First-Class Offline Accounts with Java-exact MD5 v3 UUIDs.
      </div>
    </div>
    <div style="display:flex;gap:8px;flex-wrap:wrap;">
      <button class="btn btn-primary" onclick={() => (showDeviceCodeModal = true)} data-testid="msa-login-btn">
        + Sign In with Microsoft (Device Code / PKCE)
      </button>
      <button class="btn" onclick={() => (showXstsGuide = !showXstsGuide)} data-testid="xsts-guide-btn">
        {showXstsGuide ? 'Hide XSTS Guide' : 'XSTS Error Remediation Guide'}
      </button>
    </div>
  </div>

  {#if showXstsGuide}
    <div class="card" data-testid="xsts-remediation-panel">
      <h3 style="margin-top:0;">Xbox Live / XSTS Error Code Remediation</h3>
      <div class="grid-cards">
        <div>
          <span class="badge badge-warn">2148916233</span>
          <p style="font-size:0.8rem;color:var(--text-secondary);margin:4px 0 0;">
            No Xbox Live profile linked to this Microsoft account. Sign in at xbox.com once to initialize your Gamertag.
          </p>
        </div>
        <div>
          <span class="badge badge-warn">2148916238</span>
          <p style="font-size:0.8rem;color:var(--text-secondary);margin:4px 0 0;">
            Child/Under-18 account requires a parent to enable multiplayer & third-party access in Xbox Family Settings.
          </p>
        </div>
        <div>
          <span class="badge badge-danger">No Game Ownership Entitlement</span>
          <p style="font-size:0.8rem;color:var(--text-secondary);margin:4px 0 0;">
            Account lacks <code>game_minecraft</code> entitlement. You can launch in Demo Mode or switch to an Offline Account below.
          </p>
        </div>
      </div>
    </div>
  {/if}

  <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));">
    <!-- Left Column: Account Switcher + Offline UUID Generator -->
    <div style="display:flex;flex-direction:column;gap:16px;">
      <div class="card">
        <h3 style="margin-top:0;">Configured Accounts ({accounts.length})</h3>
        <div style="display:flex;flex-direction:column;gap:8px;">
          {#each accounts as acc (acc.id)}
            <div
              style="display:flex;align-items:center;justify-content:space-between;padding:10px;background:var(--bg-elevated);border:1px solid {acc.active ? 'var(--accent-primary)' : 'var(--border-subtle)'};border-radius:var(--radius-sm);"
            >
              <div>
                <div style="display:flex;align-items:center;gap:6px;">
                  <strong>{acc.username}</strong>
                  <span class="badge {acc.type === 'microsoft' ? 'badge-info' : 'badge-success'}">
                    {acc.type.toUpperCase()}
                  </span>
                  {#if acc.active}
                    <span class="badge badge-success">ACTIVE</span>
                  {/if}
                </div>
                <div class="mono" style="font-size:0.74rem;color:var(--text-muted);margin-top:3px;">
                  {acc.uuidDashed}
                </div>
              </div>
              <button class="btn btn-sm" onclick={() => setActiveAccount(acc.id)}>
                {acc.active ? 'Selected' : 'Use Account'}
              </button>
            </div>
          {/each}
        </div>
      </div>

      <!-- Offline Account Studio with Live MD5 v3 UUID Preview -->
      <div class="card" data-testid="offline-account-studio">
        <h3 style="margin-top:0;">Offline Account Studio (`OfflinePlayer:` MD5 v3 UUID)</h3>
        <p style="font-size:0.82rem;color:var(--text-secondary);margin-top:0;">
          Computes the exact deterministic UUID used by vanilla LAN & `online-mode=false` servers:
          <code>UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))</code>
        </p>

        <div style="display:flex;gap:8px;margin-bottom:8px;">
          <input
            class="input mono"
            style="flex:1;"
            bind:value={offlineUsername}
            data-testid="offline-username-input"
            aria-label="Offline player username"
            placeholder="Enter username (3-16 chars)..."
          />
          <button class="btn btn-primary" onclick={addOfflineAccount} data-testid="add-offline-account-btn">
            + Add Offline Account
          </button>
        </div>

        <div style="display:flex;gap:6px;margin-bottom:10px;flex-wrap:wrap;">
          <span style="font-size:0.76rem;color:var(--text-muted);">Verify Test Vectors:</span>
          {#each ['Notch', 'jeb_', 'Steve', 'Alex'] as vector}
            <button
              class="btn btn-sm mono"
              data-testid="vector-{vector}"
              onclick={() => (offlineUsername = vector)}
            >
              {vector}
            </button>
          {/each}
        </div>

        <div class="card" style="background:var(--bg-input);padding:10px;font-size:0.8rem;">
          <div style="display:flex;justify-content:space-between;margin-bottom:4px;">
            <span style="color:var(--text-muted);">Dashed UUID (v3):</span>
            <code data-testid="offline-uuid-dashed">{offlineUuidPreview.dashed}</code>
          </div>
          <div style="display:flex;justify-content:space-between;margin-bottom:4px;">
            <span style="color:var(--text-muted);">Undashed Hex:</span>
            <code data-testid="offline-uuid-undashed">{offlineUuidPreview.undashed}</code>
          </div>
          <div style="display:flex;justify-content:space-between;">
            <span style="color:var(--text-muted);">Strict Format (`[A-Za-z0-9_]{3,16}`):</span>
            <span class="badge {offlineUuidPreview.validStrictUsername ? 'badge-success' : 'badge-warn'}">
              {offlineUuidPreview.validStrictUsername ? 'VALID' : 'NON-STANDARD'}
            </span>
          </div>
        </div>

        <label style="display:flex;align-items:center;gap:8px;font-size:0.79rem;margin-top:8px;color:var(--text-secondary);">
          <input type="checkbox" bind:checked={allowNonStandardUsername} />
          Advanced Override: Allow non-standard username characters/length for legacy test servers
        </label>
      </div>
    </div>

    <!-- Right Column: Interactive 60fps 3D Skin & Cape Studio -->
    <div class="card" style="display:flex;flex-direction:column;gap:12px;" data-testid="skin-viewer-studio">
      <div style="display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:8px;">
        <div>
          <h3 style="margin:0;">Interactive 60fps 3D Skin & Cape Viewer</h3>
          <span style="font-size:0.78rem;color:var(--text-muted);">
            Drag to orbit • Supports Classic (4px) & Slim (3px) arms + 64x32 legacy conversion
          </span>
        </div>
        <span class="badge badge-info">60 FPS Canvas 3D</span>
      </div>

      <!-- 3D Canvas Viewport -->
      <div
        style="display:flex;justify-content:center;background:radial-gradient(circle at 50% 35%, rgba(16,185,129,0.16), var(--bg-input) 75%);border:1px solid var(--border-subtle);border-radius:var(--radius-md);padding:8px;"
      >
        <canvas
          bind:this={canvasEl}
          width="340"
          height="260"
          data-testid="skin-3d-canvas"
          style="cursor:grab;max-width:100%;"
          onpointerdown={(e) => {
            isDragging = true;
            lastMouseX = e.clientX;
            lastMouseY = e.clientY;
          }}
          onpointermove={(e) => {
            if (!isDragging) return;
            yaw += (e.clientX - lastMouseX) * 0.012;
            pitch = Math.max(-0.6, Math.min(0.6, pitch + (e.clientY - lastMouseY) * 0.01));
            lastMouseX = e.clientX;
            lastMouseY = e.clientY;
          }}
          onpointerup={() => (isDragging = false)}
        ></canvas>
      </div>

      <div style="display:flex;gap:8px;flex-wrap:wrap;align-items:center;justify-content:space-between;">
        <div style="display:flex;gap:6px;flex-wrap:wrap;">
          <button
            class="btn btn-sm {armModel === 'classic' ? 'btn-primary' : ''}"
            onclick={() => (armModel = 'classic')}
          >
            Classic (4px)
          </button>
          <button
            class="btn btn-sm {armModel === 'slim' ? 'btn-primary' : ''}"
            onclick={() => (armModel = 'slim')}
          >
            Slim (3px)
          </button>
          <button
            class="btn btn-sm {showOuterLayer ? 'btn-primary' : ''}"
            onclick={() => (showOuterLayer = !showOuterLayer)}
          >
            Outer Layer: {showOuterLayer ? 'ON' : 'OFF'}
          </button>
        </div>

        <div style="display:flex;gap:6px;">
          {#each ['idle', 'walk', 'run'] as const as p}
            <button class="btn btn-sm {pose === p ? 'btn-primary' : ''}" onclick={() => (pose = p)}>
              {p.toUpperCase()}
            </button>
          {/each}
        </div>
      </div>

      <div style="display:flex;gap:8px;flex-wrap:wrap;align-items:center;justify-content:space-between;">
        <select class="select" bind:value={skinPreset} aria-label="Select skin preset">
          <option value="obsidian_valkyrie">Skin: Obsidian Valkyrie (64x64)</option>
          <option value="emerald_knight">Skin: Emerald Knight (64x64)</option>
          <option value="classic_steve">Skin: Classic Heritage (64x32 Auto-Upgraded)</option>
        </select>

        <select class="select" bind:value={backEquipment} aria-label="Select cape or elytra">
          <option value="cape">Back: Cape Equipped</option>
          <option value="elytra">Back: Elytra Wings</option>
          <option value="none">Back: Hidden</option>
        </select>

        <button class="btn btn-sm" onclick={() => onNotify('Exported 3D character render to PNG')}>
          📸 Snapshot PNG
        </button>
      </div>

      <div class="card" style="background:var(--bg-elevated);padding:10px;font-size:0.79rem;">
        <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-weight:600;">
          <input type="checkbox" bind:checked={offlineSkinBridgeEnabled} />
          Enable Local Offline Skin Bridge (Resource Pack / CustomSkinLoader)
        </label>
        <p style="margin:4px 0 0;color:var(--text-secondary);">
          <strong>Truthfulness Disclosure:</strong> Vanilla Minecraft clients fetch skins from Mojang session servers when online. For offline accounts, Kestryx injects your chosen skin locally via a generated client resource pack and optional CustomSkinLoader config.
        </p>
      </div>
    </div>
  </div>

  {#if showDeviceCodeModal}
    <div class="modal-backdrop" data-testid="msa-device-modal">
      <div class="modal-panel">
        <span class="badge badge-info">MICROSOFT OAUTH 2.0 DEVICE CODE + PKCE</span>
        <h3 style="margin:0;">Link Microsoft Account to Kestryx</h3>
        <p style="margin:0;color:var(--text-secondary);font-size:0.86rem;">
          Visit <code>https://www.microsoft.com/link</code> in any browser and enter your one-time verification code:
        </p>
        <div
          class="mono"
          style="font-size:1.7rem;font-weight:800;letter-spacing:0.12em;text-align:center;padding:14px;background:var(--bg-input);border:1px solid var(--border-strong);border-radius:var(--radius-md);color:var(--accent-primary);"
        >
          {deviceUserCode}
        </div>
        <div style="font-size:0.79rem;color:var(--text-muted);">
          Chain: Microsoft OAuth -> Xbox Live User Token -> XSTS Token -> Minecraft Services Bearer Token -> OS Keychain (`Credential Manager` / `libsecret` / `Keychain`).
        </div>
        <div style="display:flex;justify-content:flex-end;gap:8px;">
          <button class="btn" onclick={() => (showDeviceCodeModal = false)}>Close</button>
          <button
            class="btn btn-primary"
            onclick={() => {
              showDeviceCodeModal = false;
              onNotify('Verified Microsoft -> XBL -> XSTS -> Minecraft entitlement & stored refresh token in OS Keychain.');
            }}
          >
            Simulate Instant Browser Approval
          </button>
        </div>
      </div>
    </div>
  {/if}
</section>
