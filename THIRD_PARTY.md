# Third-Party Content, Services, and Trademarks

ModpackPilot is an independent, open-source server management tool. This document
explains what ModpackPilot does and does not distribute, which third-party services it
talks to, and whose terms apply to the content it installs.

**Short version:** ModpackPilot ships no Minecraft content, no mods, and no modpacks.
It is a client that downloads files *to your machine, on your instruction, from the
official servers of the projects that publish them*. Every file it installs remains
governed by the licence of whoever published it.

---

## 1. What this repository contains

This repository contains only ModpackPilot's own source code and its own icons and
assets, licensed under [MIT](LICENSE).

This repository does **not** contain, and must never contain:

- Minecraft client or server JARs, libraries, or assets
- Forge, NeoForge, or Fabric installers or artifacts
- Any mod, modpack, resource pack, datapack, world, or config authored by a third party
- Any cached, vendored, or mirrored copy of a file retrieved from FTB, Modrinth, Mojang,
  or a mod author's own distribution
- Feed the Beast, Modrinth, Mojang, or Microsoft logos or brand assets

## 2. How ModpackPilot obtains files

ModpackPilot never hosts, mirrors, proxies, re-serves, or caches third-party content on
any infrastructure it controls. It has no server-side component at all.

Every download is made by the copy of ModpackPilot running on your own computer, using
the download URL that the upstream service itself publishes for that file, over a direct
connection from your machine to that service. ModpackPilot acts as your user agent — the
same role a web browser plays when you click a download link. The bytes travel from the
publisher to you. No copy passes through the project or its maintainers.

Services contacted, and the endpoints used:

| Service | Endpoint | Purpose |
| --- | --- | --- |
| Feed the Beast | `api.feed-the-beast.com/v1/modpacks/public` | Modpack search, version metadata, and the per-file download URLs FTB publishes |
| Modrinth | `api.modrinth.com/v2` | Project search, version metadata, and `.mrpack` retrieval |
| MinecraftForge | `maven.minecraftforge.net` | Official Forge installer artifacts |
| NeoForged | `maven.neoforged.net` | Official NeoForge installer artifacts |
| FabricMC | `meta.fabricmc.net` | Official Fabric server launcher artifacts |

The Minecraft server JAR itself is downloaded by the mod loader's own official installer,
run locally on your machine. ModpackPilot does not retrieve, bundle, or redistribute any
Mojang binary.

When FTB's manifest supplies alternate ("mirror") URLs for a file — which FTB does for
content it does not itself redistribute — ModpackPilot uses the URLs FTB provides, in the
order FTB provides them. It does not attempt to source files from anywhere other than the
locations the upstream manifest names.

## 3. Feed the Beast

FTB modpacks, and the mods within them, are **not** licensed under this repository's MIT
licence and are **not** redistributed by this project.

Your use of any FTB modpack or mod is governed by Feed the Beast Limited's
[Modpack/Mods Policy](https://www.feed-the-beast.com/policies/modpacks-mods-policy) and
[Terms and Conditions](https://www.feed-the-beast.com/policies/terms-and-conditions), not
by anything in this repository. In particular, FTB grants a download licence that is
**non-transferable and carries no right to sublicense**, and is limited to your own
personal use. Read those terms before installing an FTB pack. Individual mods inside a
pack additionally carry their authors' own licences, which may be more restrictive than
FTB's.

ModpackPilot is not affiliated with, endorsed by, sponsored by, or connected to Feed the
Beast Limited. "Feed the Beast", "FTB", "StoneBlock", and the FTB logo are UK registered
trade marks of Feed the Beast Limited. This project uses the words "Feed the Beast" and
"FTB" only descriptively, to identify which service a feature talks to. No FTB logo or
brand asset is used or bundled.

## 4. Modrinth

Modrinth projects and the mods within them are **not** licensed under this repository's
MIT licence and are **not** redistributed by this project.

Content retrieved from Modrinth is governed by the
[Modrinth Terms of Use](https://modrinth.com/legal/terms) and by the individual licence
each project author selected, which is published on that project's Modrinth page. Many
Modrinth projects are permissively licensed; many are not. It is your responsibility to
comply with the licence of each mod you install.

ModpackPilot identifies itself to the Modrinth API with a unique `User-Agent` naming the
project and linking to its repository, as
[Modrinth's API documentation requires](https://docs.modrinth.com/api/), and stays well
inside the published rate limit.

ModpackPilot is not affiliated with, endorsed by, sponsored by, or connected to Rinth,
Inc. "Modrinth" and the Modrinth logo are trademarks of Rinth, Inc. This project uses the
word "Modrinth" only descriptively. No Modrinth logo or brand asset is used or bundled.

## 5. Minecraft, Mojang, and Microsoft

NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR
MICROSOFT.

ModpackPilot is an independent tool. Minecraft is a trademark of Mojang AB / Microsoft
Corporation. Running a Minecraft server requires you to accept the
[Minecraft End User Licence Agreement](https://aka.ms/MinecraftEULA). ModpackPilot does
not grant you any rights in Minecraft and does not distribute any part of it.

## 6. Mod loaders

Forge, NeoForge, and Fabric installers are downloaded from their projects' official Maven
repositories and executed locally. They are distributed under their own licences (LGPL
2.1 for Forge and NeoForge; Apache 2.0 for Fabric Loader) by their own projects, not by
ModpackPilot.

## 7. Bundled open-source dependencies

ModpackPilot is built on Tauri, Rust, React, and their respective dependency trees. Those
components are distributed under their own open-source licences. A full dependency
licence manifest can be regenerated with `cargo license` and a Node licence checker.

## 8. Reporting a licensing concern

If you are a rights holder and believe ModpackPilot handles your content improperly, or
if you represent Feed the Beast Limited or Rinth, Inc. and object to how this project
uses your service or name, please open an issue on the repository or contact the
maintainers. We will act on well-founded requests promptly, including removing an
integration entirely.
