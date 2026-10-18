# Contributing to Lightyear

First off, thank you for taking the time to contribute! 💙
Every contribution helps, whether it's a typo fix, a bug report, a UI polish or a whole new feature.

## 📋 Table of contents

- [Code of Conduct](#code-of-conduct)
- [Ways to contribute](#ways-to-contribute)
- [Development setup](#development-setup)
- [Project layout](#project-layout)
- [Coding guidelines](#coding-guidelines)
- [Commit messages](#commit-messages)
- [Pull request process](#pull-request-process)
- [Releasing (maintainers)](#releasing-maintainers)

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.

## Ways to contribute

- 🐛 **Report bugs.** Use the [bug report form](https://github.com/firearz/lightyear-launchpad/issues/new?template=bug_report.yml) and attach `latest.log` if possible.
- 💡 **Suggest features.** Use the [feature request form](https://github.com/firearz/lightyear-launchpad/issues/new?template=feature_request.yml).
- 📝 **Improve docs.** README, FAQ entries, code comments.
- 🎨 **Polish the UI.** New instance icons, animations, accessibility.
- 🔧 **Write code.** Look for issues labelled [`good first issue`](https://github.com/firearz/lightyear-launchpad/labels/good%20first%20issue) or [`help wanted`](https://github.com/firearz/lightyear-launchpad/labels/help%20wanted).

> [!TIP]
> Before starting a large change, please open an issue to discuss it first, so nobody spends time on work that might not be merged.

## Development setup

**Prerequisites:** Windows 10/11, Python 3.10+, Git.

```powershell
# 1. Fork the repo on GitHub, then clone your fork
git clone https://github.com/<your-username>/lightyear-launchpad.git
cd lightyear-launchpad

# 2. Create a virtual environment
python -m venv .venv
.venv\Scripts\activate

# 3. Install runtime + dev dependencies
pip install -r requirements-dev.txt

# 4. Run the launcher
python main.py
```

Useful commands:

| Task | Command |
| --- | --- |
| Lint | `ruff check .` |
| Auto-fix lint | `ruff check . --fix` |
| Build `.exe` | `pyinstaller lightyear.spec --noconfirm` |

## Project layout

| Path | Responsibility |
| --- | --- |
| `main.py` | Entry point: DPI policy, fonts, app icon, window placement |
| `lightyear/core.py` | Backend: `LauncherStore`, download decision matrix, Java resolution, install and launch `QThread` workers |
| `lightyear/main_window.py` | `LightyearMainWindow`: page routing, wiring UI signals to backend actions |
| `lightyear/ui_components.py` | Frosted-glass widgets, dialogs, Home / Installed / Help pages, FAQ database |
| `assets/` | Logo, backdrop, fonts, pixel-art instance icons |

**Key rules of the architecture:**

- Never block the UI thread. Network and disk work belongs in a `QThread` worker that reports back through `Signal`s.
- Instances are isolated (`instances/<id>/`), while game files are shared (`versions/`, `libraries/`, `assets/`). Don't break that boundary.
- Load bundled files through `ASSETS_DIR` / `PROJECT_ROOT` from `core.py`, so they also work inside the PyInstaller build.

## Coding guidelines

- Follow **PEP 8**, with a max line length of **120**.
- Use **type hints** on all new functions (`from __future__ import annotations` is already used).
- Write **docstrings** for public classes and non-trivial functions.
- Keep UI strings short and friendly. Match the existing tone.
- Run `ruff check .` before pushing. CI runs the same check.
- If you add a user-facing feature, consider adding an entry to `FAQ_DATABASE` in `ui_components.py`.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```text
feat: add Modrinth mod search
fix: correct Java 21 detection for 1.20.5
docs: add screenshots to README
refactor(core): split download matrix into its own module
```

Common types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`.

## Pull request process

1. Create a branch from `main`: `git checkout -b feat/my-feature`.
2. Make your changes and test them manually (create an instance, launch, quit).
3. Make sure `ruff check .` passes.
4. Update `README.md` / `CHANGELOG.md` (under **Unreleased**) if relevant.
5. Open a PR and fill in the template. Include **screenshots** for UI changes.
6. A maintainer will review it. Please be patient and open to feedback. 🙂

## Releasing (maintainers)

1. Move the **Unreleased** entries in `CHANGELOG.md` under a new version heading.
2. Bump `__version__` in `lightyear/__init__.py`.
3. Commit, then tag and push:
   ```powershell
   git tag v1.1.0
   git push origin main --tags
   ```
4. The **Release** workflow builds the `.exe`, the portable `.zip` and `SHA256SUMS.txt`, then publishes the GitHub Release.

Thanks again for helping make Lightyear better! 🚀
