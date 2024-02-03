# DMM, the Declarative Music Manager (& Player)

Tired of being dependant on servecies like spotify? Former user of MPD, but hate having to manually collect the audio files?
Use NixOS, and want to edit even more configs? Hate YouTube, and want to legally* make them mad?

If any of these things apply to you, you should try DMM!

<small>*I am not a lawyer, but yt-dlp hasn't gotten taken down yet!</small>

## Table of Contents

- [Explainer/Usage](#how-it-works)
- [Installation](#installation)
- [Getting Started](#getting-started)
- [Misc](#misc)

**WARNING: the main branch of DMM can move quickly, and the documentation here may be inacurate.
the [stable branch](https://git.fawkes.io/mtnash/dmm/src/branch/stable) has the latest stable release,
and it is recommended to look there instead**

## How It Works

DMM, like Nix, is *declarative*. Using DMM to play music happens in 3 steps

For *general configuration* see [getting started](#getting-started), [the default config](/assets/dmm.default.ron) and [the example config](/examples/dmm.ron)

### 1) Declare

The first step is to define the music that you want to listen to in a config file.

DMM Configuration is organized as follows (examples can be found [here](/examples/))

Music Directory: the 'root' directory where DMM's files live. In this there are 3 items
- `dmm.ron`: This is the main configuration. Here you can create custom keybindings, and change settings
- `playlists`: This directory contains your playlists, one file per playlist.
- `sources`: This directory contains 'sources' for music. This is explained more in depth later.

You may want to use git to manage any changes you make to your playlists, but remember to add `run/` and `cache/` to your `.gitignore`!

#### 1.1) Playlists

Each playlist is defined with a `<playlist-name>.ron` file in the `playlists/` directory.
A playlist file contains the following:

- The name of the playlist
- Imports: any *imported* sources for the playlist (from the `sources/` directory)
- Sources: any *non-imported* sources for the playlist (declared inline)
- Tracks: Definitions of each track, including which source to use and the input for that source

#### 1.2) Sources

A music player is good, but useless without a way to *get* the music to play. (*cough* *cough* mpd)

Here, DMM provides a rather open-ended solution, implemented through sources. Currently only one exists, which is the `Shell` source.
This source runs a shell command to fetch the audio, allowing for integration with many external programs such as [`yt-dlp`](https://github.com/yt-dlp/yt-dlp).

Here is an example of using the [example yt-dlp source](/examples/sources/yt-dlp.ron) to download
the song Let It Snow from the link <youtube.com/watch?v=2TA3IKH8Y5c>

```ron
Track(
    meta: Meta(
        name: "Let It Snow!",
        artist: "Dean Martin",
    ),
    src: "yt",
    // This is the portion of the youtube link after `watch?v=`
    input: "2TA3IKH8Y5c",
)
```

### 2) Fetch

After you have defined a playlist, DMM needs to collect the audio from the sources, and save it in a local cache.

This functionality is currently extremely simple, with a few limitations that will be explained later.

To download the playlist `Classic Christmas Songs`, navigate to the root of the music directory and run the command

```zsh
dmm download "Christmas Songs"
```

the third parameter, (here "Christmas Songs") is used to search all playlist in the `playlists/` directory for ones with similar names.
it can be a part of, or similar to the playlist name (the program will ask you to check the playlist it chose was correct before continuing)

#### 2.1) Updating the Cache

If new songs are added or a source changes, songs need to be re-downloaded. this is as simple as running `dmm download` again.
- changing a sources *name*, or the name/artist field of a track will not trigger this
- changing the command/parameters of a source *at all* will trigger a re-download. there is currently no way to tell dmm to not do this.

#### 2.2) Garbage Collection

When songs are no longer referenced by a playlist, they remain in the cache, taking up space.
To clean the cache (delete all audio not *currently referenced*), run `dmm gc`

### 3) Enjoy!

Time to listen to your ~hard earned~ music! Navigate to your music directory, and run the following command

```zsh
dmm player "Christmas Songs"
```

The `player` command uses the same scheme as `download` to find the playlist that you give the name of, see that section for
details on how to specify the name of your playlist.

And remember piracy, especially from music publishers, is a victimless crime!

#### 3.1) Music Player UI

Navbar (the top of the screen)
- shuffle play (on/off)
- repeat (on/single/off)
- stop/play/pause
- `<time in song> -> <length of song>`
- `<song #>/<# of songs in playlist>`
- `<track title>`

On the left:
- Playlist information
- Track information
- **Currently configured keybindings**

On the right:
- Track selection: lists track # and title.
 - use the keybindings shown on the left of the screen to navigate!

## Installation

**DMM is built on, and for, linux.** It may work on windows, but you will need to build from source

Currently no {nixpkgs,AUR,cargo} package exists (coming soon?), so installation is only supported through nix flakes.

**IMPORTANT: In all examples using Nix, the URL contains a placeholder, `<version-name>` which must be replaced with one of the following**
- `stable` - will automatically download the latest stable release
- `nightly` - will get the latest nightly release
 - nightly releases will run, but may have bugs / breaking changes / features that break your cache
- `main` - will download the latest commit
 - **DO NOT USE**: the `main` branch will often be broken!
- a specific version from the releases tab, eg `v<x>.<y>.<z>` <- this is not the actual latest release
  - **not recommended after 1.0.0: if you choose this method, you need to update the version tag in order to update DMM as well as using `nix flake update` or similar!**

### 0) From crates.io

**not recommended: if you can install through a real package manager, please do**

DMM is packaged on crates.io, if you have cargo and the requred dependencies installed, run `cargo install dmm`

### 1) NixOS (Flake)

To install the `dmm` flake, add it to your system configurations `inputs`

```nix
inputs = {
  nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  # -- snip --
  
  # -- add this part --
  dmm = {
    url = "tarball+https://git.fawkes.io/mtnash/dmm/archive/<version-name>.tar.gz";
    inputs.nixpkgs.follows = "nixpkgs";
  };
};
```

Then add the apropreate dmm package to your `packages` array

```nix
packages = with pkgs; [
  inputs.dmm.packages.x86_64-linux.default
  # -- snip --
];
```

For more information on how to do this, I found [this blog post](https://www.falconprogrammer.co.uk/blog/2023/02/nixos-22-11-flakes/) helpful

### 2) Nix Profile (Flake)

To install `dmm` non-declaratively using `nix profile`, run the following:

```zsh
nix profile install tarball+https://git.fawkes.io/mtnash/dmm/archive/<version-name>.tar.gz
```

#### 2.1) Nix Build (Flake)

To build, but not install `dmm` you can use `nix build`:

```zsh
nix build tarball+https://git.fawkes.io/mtnash/dmm/archive/<version-name>.tar.gz
```

The executable will be located in `./result/dmm`

### 3) Build From Source

Dependancies:
- `cargo`
- `rustc` nightly
- `clang` + `mold` (*linux only*)
- `alsa` (*linux only*)

If all dependancies are built correctly, `cargo build --release` is all that needs to be done, your binary
will end up in `target/release/dmm`

## Getting Started

First, you should have `dmm` installed and (optionally, but the example playlist uses it) [`yt-dlp`](https://github.com/yt-dlp/yt-dlp)

This guide (and DMM in general) assumes that you have some experience working with the linux command line,
If you see any improvements, open an issue/PR!

You should also read [the how it works section](#how-it-works), this will help you understand how to create new playlists and sources yourself.

### Creating a Music Directory

The first step is to decide where to keep your music. You most likely want a directory seperate from the rest of your music (if you have any).
This guide will assume you are using `~/music`.

After creating the `music` directory, `cd` into it and run `dmm init`.
This will create the basic layout of the music directory, as well as a playlist named `example`.
It will also import the [example `yt-dlp` source](/examples/sources/yt-dlp.ron).

### Play the Example Playlist

Before you can play the playlist, you have to fetch the tracks from their sources.
in `~/music`, run `dmm download playlist 'example'`. the download command will search for that name (it can be incomplete)
and download that playlists tracks. you can also use `dmm download all` to download all playlists at once.

Now that it is downloaded, you can play it. run `dmm player 'example'` to launch the player.

In the center you can see a list of the tracks in the current playlist.
You can use `j` and `k` to scroll down this list, and `enter` to play a track.

*All of these keybindings and more are listed on the lower left!*

### Make Your Own!

Take a look at [How It Works](#how-it-works) to see how to create your own playlists, and even sources!

## Misc

### Use DMM without leaving your ${directory}

To tell DMM to use `<path>` as the path for your music directory, instead of the current directory,
create a `.dmm-link.ron` file with the following contents

```ron
Link(
    music_directory: "<path>"
)
```

