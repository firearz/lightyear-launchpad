# Third-Party Notices

Lightyear is MIT-licensed. It depends on, or bundles, the following third-party components.

## Bundled with the application

| Component | License | Source |
| --- | --- | --- |
| Lato (Regular, Bold, Black) | SIL Open Font License 1.1 | https://fonts.google.com/specimen/Lato |
| Playfair Display | SIL Open Font License 1.1 | https://fonts.google.com/specimen/Playfair+Display |

## Python dependencies (included in release binaries)

| Package | License | Source |
| --- | --- | --- |
| PySide6 / Qt 6 | LGPL-3.0 | https://doc.qt.io/qtforpython-6/ |
| minecraft-launcher-lib | BSD-2-Clause | https://codeberg.org/JakobDev/minecraft-launcher-lib |
| Pillow | MIT-CMU (HPND) | https://python-pillow.org/ |
| PyInstaller bootloader | GPL-2.0 with bootloader exception | https://pyinstaller.org/ |

### Qt / PySide6 (LGPL-3.0)

Release binaries link to the Qt libraries dynamically. You may replace the bundled Qt / PySide6 libraries with
your own compatible build. With the **portable `.zip`** release, swap the DLLs inside the folder. With the
single `.exe`, run Lightyear from source (`python main.py`) with your own PySide6 installation.
The source code for Qt and PySide6 is available at https://code.qt.io/.

## Minecraft

Lightyear does **not** include or redistribute any Minecraft game files. All game assets, libraries and
client JARs are downloaded at runtime directly from Mojang's official servers (and from the official Fabric,
Quilt and Forge distribution servers for mod loaders).

NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.
