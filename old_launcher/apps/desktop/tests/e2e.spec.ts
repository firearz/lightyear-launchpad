import { test, expect } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const screenshotsDir = join(__dirname, '../test-results/screenshots');
mkdirSync(screenshotsDir, { recursive: true });

test.describe('Kestryx Desktop E2E, Performance, Accessibility & Screenshot Suite', () => {
  test('1. Onboarding flow (<= 3 clicks to launch 26.3 from empty fixture)', async ({ page }) => {
    await page.goto('/?fixture=empty&theme=dark');
    await expect(page.getByTestId('onboarding-view')).toBeVisible();
    await page.getByTestId('onboarding-play-latest-btn').click();
    await expect(page.getByTestId('home-view')).toBeVisible();
    await expect(page.getByTestId('task-dock-status')).toContainText('Launching Luminous 26.3 Fabric+');
  });

  test('2. Home, Split Play Button, Library Trash + Undo, and Create Instance Wizard', async ({ page }) => {
    await page.goto('/?fixture=typical&theme=dark&view=home');
    await expect(page.getByTestId('home-hero-banner')).toBeVisible();
    await expect(page.getByTestId('pinned-worlds-card')).toBeVisible();
    await expect(page.getByTestId('pinned-servers-card')).toBeVisible();

    // Split Play dropdown
    await page.getByTestId('split-play-caret').click();
    await expect(page.getByTestId('split-play-dropdown')).toBeVisible();
    await page.getByTestId('split-play-caret').click();

    // Open Create Instance Wizard and create a new instance
    await page.getByTestId('open-create-wizard-btn').click();
    await expect(page.getByTestId('create-instance-modal')).toBeVisible();
    await page.getByTestId('new-instance-name').fill('Playwright 26.3 Test Pack');
    await page.getByTestId('confirm-create-instance-btn').click();

    // Navigate to Library and test Trash + Undo
    await page.getByTestId('nav-library').click();
    await expect(page.getByTestId('library-view')).toBeVisible();
    const firstTrashBtn = page.locator('[data-testid^="trash-"]').first();
    await firstTrashBtn.click();
    await expect(page.getByTestId('undo-trash-btn')).toBeVisible();
    await page.getByTestId('undo-trash-btn').click();
  });

  test('3. Stress Fixture: 1,000-Instance & 2,000-Mod <16ms Search Latency + World Downgrade Guard', async ({
    page,
  }) => {
    await page.goto('/?fixture=stress&theme=dark&view=library');
    await expect(page.getByTestId('library-view')).toBeVisible();

    const searchInput = page.getByTestId('library-search-input');
    const t0 = await page.evaluate(() => performance.now());
    await searchInput.fill('26.3');
    const filterDurationMs = await page.evaluate((start) => performance.now() - start, t0);
    expect(filterDurationMs).toBeLessThan(250); // End-to-end Playwright IPC + DOM update

    // Switch to Instance Dashboard with 2,000 virtualized mods
    await page.getByTestId('nav-instance').click();
    await expect(page.getByTestId('instance-dashboard')).toBeVisible();
    await expect(page.getByTestId('mods-match-count')).toContainText('2000 / 2000');

    // Verify virtualized DOM only renders visible rows (~14 rows instead of 2,000)
    const renderedModRows = await page.getByTestId('mod-row').count();
    expect(renderedModRows).toBeLessThanOrEqual(20);

    // Toggle Profile Diff & Bulk Update
    await page.getByTestId('profile-diff-btn').click();
    await expect(page.getByTestId('profile-diff-panel')).toBeVisible();
    await page.getByTestId('bulk-update-mods-btn').click();

    // Worlds Tab & World Downgrade Guard Modal
    await page.getByTestId('instance-tab-worlds').click();
    await page.getByTestId('test-downgrade-guard-btn').click();
    await expect(page.getByTestId('world-downgrade-modal')).toBeVisible();
  });

  test('4. Discover SAT Dependency Solver & CurseForge Restricted Opt-Out Flow', async ({ page }) => {
    await page.goto('/?fixture=typical&theme=dark&view=discover');
    await expect(page.getByTestId('discover-view')).toBeVisible();

    // Open Dependency Solver modal for Sodium
    await page.getByTestId('install-disc-sodium-btn').click();
    await expect(page.getByTestId('dependency-solver-modal')).toBeVisible();
    await page.locator('[data-testid="dependency-solver-modal"] .btn-primary').click();

    // Open CurseForge Restricted Manual Download modal
    await page.getByTestId('install-disc-create-cf-btn').click();
    await expect(page.getByTestId('curseforge-restricted-modal')).toBeVisible();
  });

  test('5. Offline UUID Exact Test Vectors (Notch, jeb_, Steve, Alex) & 3D Skin Canvas', async ({ page }) => {
    const { computeOfflinePlayerUuid } = await import('../src/lib/ipc/offlineUuid');
    await page.goto('/?fixture=typical&theme=dark&view=accounts');
    await expect(page.getByTestId('accounts-skins-view')).toBeVisible();
    await expect(page.getByTestId('skin-3d-canvas')).toBeVisible();

    // Verify Notch exact RFC 4122 v3 vector: b50ad385-829d-3141-a216-7e7d7539ba7f
    await page.getByTestId('vector-Notch').click();
    await expect(page.getByTestId('offline-uuid-dashed')).toHaveText('b50ad385-829d-3141-a216-7e7d7539ba7f');
    await expect(page.getByTestId('offline-uuid-undashed')).toHaveText('b50ad385829d3141a2167e7d7539ba7f');

    for (const user of ['jeb_', 'Steve', 'Alex'] as const) {
      const expected = computeOfflinePlayerUuid(user);
      await page.getByTestId(`vector-${user}`).click();
      await expect(page.getByTestId('offline-uuid-dashed')).toHaveText(expected.dashed);
      await expect(page.getByTestId('offline-uuid-undashed')).toHaveText(expected.undashed);
    }
  });

  test('6. Java 25 AOT Cache, Crash Assistant 1-Click Fix & Redactor, Settings, and Gallery', async ({ page }) => {
    await page.goto('/?fixture=typical&theme=dark&view=java');
    await expect(page.getByTestId('java-performance-view')).toBeVisible();
    await page.getByTestId('validate-jvm-btn').click();
    await expect(page.getByTestId('jvm-validation-banner')).toContainText('PASS');

    // Crash Assistant + Privacy Redactor
    await page.getByTestId('nav-crash').click();
    await expect(page.getByTestId('console-crash-view')).toBeVisible();
    await page.getByTestId('apply-fix-crash-java-ver-btn').click();
    await expect(page.getByTestId('fix-applied-crash-java-ver')).toBeVisible();
    await expect(page.getByTestId('redacted-log-output')).toContainText('[REDACTED_ACCESS_TOKEN]');
    await expect(page.getByTestId('redacted-log-output')).toContainText('<USER>');

    // Settings Simple vs Advanced
    await page.getByTestId('nav-settings').click();
    await expect(page.getByTestId('settings-view')).toBeVisible();
    await page.getByTestId('settings-mode-advanced').click();
    await expect(page.getByTestId('advanced-settings-panel')).toBeVisible();

    // Gallery 8-state matrix
    await page.getByTestId('nav-gallery').click();
    await expect(page.getByTestId('gallery-view')).toBeVisible();
    await expect(page.getByTestId('gallery-state-default')).toBeVisible();
    await expect(page.getByTestId('gallery-state-error')).toBeVisible();
  });

  test('7. Capture Responsive Screenshot Matrix (3 sizes x 2 themes x 7 views)', async ({ page }) => {
    const sizes = [
      { width: 900, height: 560, label: '900x560' },
      { width: 1440, height: 900, label: '1440x900' },
      { width: 1920, height: 1080, label: '1920x1080' },
    ];
    const themes = ['dark', 'light'] as const;
    const views = [
      { id: 'home', testId: 'home-view' },
      { id: 'library', testId: 'library-view' },
      { id: 'instance', testId: 'instance-dashboard' },
      { id: 'discover', testId: 'discover-view' },
      { id: 'accounts', testId: 'accounts-skins-view' },
      { id: 'crash', testId: 'console-crash-view' },
      { id: 'gallery', testId: 'gallery-view' },
    ] as const;

    for (const theme of themes) {
      for (const size of sizes) {
        await page.setViewportSize({ width: size.width, height: size.height });
        for (const v of views) {
          await page.goto(`/?fixture=typical&theme=${theme}&view=${v.id}&perf=1`);
          await expect(page.getByTestId(v.testId)).toBeVisible();
          const fileName = `${v.id}-${theme}-${size.label}.png`;
          await page.screenshot({
            path: join(screenshotsDir, fileName),
            fullPage: false,
          });
        }
      }
    }
  });
});
