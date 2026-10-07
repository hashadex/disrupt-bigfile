# disrupt-bigfile

A CLI tool and Rust library for inspecting, extracting and creating BigFile archives used by
[Ubisoft's Disrupt engine] in the [Watch Dogs] games.

This project is heavily based on the research done in the [Gibbed.Disrupt] project, a tool used for
modding Watch Dogs games. This project aims to be an easy-to-use, performant, compatible and
documented alternative for it.

> [!NOTE]
> This is an unofficial, fan-made project and is not affiliated with, endorsed by, or approved by
> Ubisoft. Watch Dogs, and all related trademarks, characters and intellectual property are owned
> by Ubisoft.

> [!NOTE]
> This documentation is for the CLI part of this project, meant for users and mod authors to learn
> how to manipulate BigFile archives using the CLI. If you are a developer and you want to use
> disrupt-bigfile as a library in your own projects, see the [API documentation].

[Ubisoft's Disrupt engine]: https://en.wikipedia.org/wiki/Ubisoft#Disrupt
[Watch Dogs]: https://en.wikipedia.org/wiki/Watch_Dogs
[Gibbed.Disrupt]: https://github.com/gibbed/Gibbed.Disrupt
[API documentation]: https://docs.rs/disrupt-bigfile

## Installation

### Method A: Download binary (recommended)

1. Go to the [latest release] page.
2. In the **Assets** list, click on the executable file appropriate for your platform in order to
   download it.

[latest release]: https://github.com/hashadex/disrupt-bigfile/releases/latest

### Method B: Download and compile from crates.io (for advanced users)

If you have Rust and Cargo installed, you can download the source code of disrupt-bigfile from
crates.io and compile it on your machine using `cargo install`:

```sh
$ cargo install --features "cli" disrupt-bigfile
```

## Usage

disrupt-bigfile is a command line tool, which means it can only be used from the command line.
Drag-and-drop usage like in Gibbed.Disrupt is currently not supported.

Other than that, disrupt-bigfile aims to be fully compatible with Gibbed.Disrupt. Any mod that is
meant to be installed using Gibbed.Disrupt can be installed using disrupt-bigfile (and vice versa).

### Extract archives using `unpack`

The `unpack` subcommand allows you to extract BigFile archives:

```sh
$ disrupt-bigfile unpack --fat patch.fat --dat patch.dat --output out_dir
Unpacking archive to directory 'out_dir' from...
  FAT: 'patch.fat'
  DAT: 'patch.dat'

FAT info: FAT3, Table V8, Platform Win64, Compression V5, Name hash V50
```

### Create archives using `pack`

The `pack` subcommand allows you to create BigFile archives from directories:

```sh
$ disrupt-bigfile pack out_dir --name patch
Packing directory '/home/example/out_dir' to...
  FAT: '/home/example/patch.fat'
  DAT: '/home/example/patch.dat'

FAT info: FAT3, Table V8, Platform Win64, Compression V5, Name hash V50
```

By default, the `pack` subcommand will create archives for the Windows version of Watch Dogs 1. If
you want to create an archive for a different game, use the `--preset` option. For example, if you
want to create a `patch` archive for the Windows version of Watch Dogs 2, use the `wd2-win64`
preset:

```sh
$ disrupt-bigfile pack out_dir --name patch
Packing directory '/home/example/out_dir' to...
  FAT: '/home/example/patch.fat'
  DAT: '/home/example/patch.dat'

FAT info: FAT5, Table V11, Platform Win64, Compression V6, Name hash V70, Archive hash 0xFFFFFFFFFFFFFFFF, Dependencies []
```

See the help message for `pack` for the full list of available presets and options:

```sh
$ disrupt-bigfile pack --help
```

### Inspect FAT files using `info` and `list`

The `info` subcommand displays the header metadata of a FAT file:

```sh
$ disrupt-bigfile info patch.fat
patch.fat
FAT version:         FAT3
Table version:       V8
Platform:            Win64
Compression version: V5
Name hash version:   V50
```

The `list` subcommand lists all file entries contained in a FAT without extracting anything.

```sh
$ disrupt-bigfile list patch.fat
domino/user/windycity/main_missions/act_02/mission_10/a02_m10.a02_m10.lua
generated/databases/generic/posewrinklemaskset.lib
generated/databases/generic/genericlayouts.lib
generated/databases/generic/personalcontractassassinationlayouts.lib
generated/databases/generic/musiclibrary_e12a2ae5.obj
languages/patch1_dutch.loc
generated/databases/generic/mediabroadcastemittersettings_2ce33943.obj
generated/databases/generic/intrusiondetectedsettings_2ce33943.obj
generated/databases/generic/musiclibrary_ecd4ab92.obj
generated/databases/generic/musiclibrary_4f441d88.obj
...
```

Use the `--verbose` flag to print more information about each file:

```sh
$ disrupt-bigfile list --verbose patch.fat
281446B @ 0x920A61: domino/user/windycity/main_missions/act_02/mission_10/a02_m10.a02_m10.lua
2372B @ 0x1C31987: generated/databases/generic/posewrinklemaskset.lib
1532B @ 0x18BBC59: generated/databases/generic/genericlayouts.lib
4062B @ 0x1BF114F: generated/databases/generic/personalcontractassassinationlayouts.lib
248B @ 0x1BC1C20: generated/databases/generic/musiclibrary_e12a2ae5.obj
1476B @ 0x1E44CC5: languages/patch1_dutch.loc
77B @ 0x1B3E381: generated/databases/generic/mediabroadcastemittersettings_2ce33943.obj
113B @ 0x1A51C0D: generated/databases/generic/intrusiondetectedsettings_2ce33943.obj
155B @ 0x1BC2E72: generated/databases/generic/musiclibrary_ecd4ab92.obj
151B @ 0x1BB8763: generated/databases/generic/musiclibrary_4f441d88.obj
...
```
