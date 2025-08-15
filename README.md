# Hoolamike
[![GitHub version](https://img.shields.io/github/v/tag/Niedzwiedzw/hoolamike?label=version&style=flat-square)](https://github.com/sNiedzwiedzw/hoolamike./releases/latest)
[![GitHub stars](https://img.shields.io/github/stars/Niedzwiedzw/hoolamike.svg?style=flat-square)](https://github.com/Niedzwiedzw/hoolamike./stargazers)
[![GitHub issues](https://img.shields.io/github/issues/Niedzwiedzw/hoolamike.svg?style=flat-square)](https://github.com/Niedzwiedzw/hoolamike./issues)
[![GitHub license](https://img.shields.io/github/license/Niedzwiedzw/hoolamike.svg?style=flat-square)](https://github.com/Niedzwiedzw/hoolamike./blob/dev/LICENSE)
![Discord](https://img.shields.io/discord/1320853150910906541)

> [!WARNING]  
> This software is in **alpha**. Features may break and change at any time. Absolutely no warranty is provided. 

**Hoolamike** is a high performance and cross-platform mod installation suite written in Rust.

## Features

- Simple configuration with YAML
- Wabbajack-compatible mod list installer
- (Fallout: New Vegas) Large-Address Aware Executable patcher
- (Fallout: New Vegas) .MPI (Tale of Two Wastelands, Ultimate Edition ESM Fixes) installer
- (Skyrim) Texture conversion utilities

## Games confirmed to work
- [**Stardew Valley**](https://store.steampowered.com/app/413150/)
- [**Skyrim**](https://store.steampowered.com/app/489830/)
- [**Fallout 4**](https://store.steampowered.com/app/377160/)  
- [**Fallout: New Vegas**](https://store.steampowered.com/app/22380/)  
- [**Fallout 3**](https://store.steampowered.com/app/22370/)

## Lists confirmed to work
> [!NOTE]
> All the available mod lists for the games listed _should_  work but we cannot test them all without ***your*** help. Join our [Discord](https://discord.gg/xYHjpKX3YP) today!

- [**Tuxborn**](https://github.com/Omni-guides/Tuxborn): A Wabbajack Modlist for Skyrim SE designed with the Steam Deck in mind 🐉  
- [**Wasteland Reborn**](https://github.com/Camora0/Wasteland-Reborn): modlist that aims to create a fully featured, lore friendly rebuild of Fallout 4 from the ground up.  ☢️  
- [**Magnum Opus**](https://github.com/LivelyDismay/magnum-opus): what one would describe as a "kitchen sink list." ☢️  
- [**Begin Again**](https://www.nexusmods.com/newvegas/mods/79547): modlist focused on bringing the gameplay feel of later Fallout titles into Fallout 3 and New Vegas ☢️  

Make sure to ⭐ star their repositories — their work is the backbone of this project!

## Quick start

1. [Download the latest release](https://github.com/Niedzwiedzw/hoolamike/releases/latest), or [compile from source](compiling-from-source). You can place the resulting binary wherever you'd like.

2. Add the Hoolamike binary to your $PATH

3. Hoolamike uses YAML for configuration. You can create the default configuration file like so:
    ```bash
    hoolamike print-default-config > hoolamike.yaml
    ```

   You may also use a specific configuration file 
   ```bash
   hoolamike -c /path/to/provided/file [OPTION]
   ```
4. For basic instruction, you may run the following.
   ```bash
   hoolamike --help
   ```
---
To see an annotated configuration file, click [here](/docs/example.yaml).

For more detailed usage instructions, please consult the [Documentation](/docs/).

## Compiling from source
1. Clone the repository with Git
2. Enter the resulting folder
    ```bash
    cd hoolamike
    ```
2. Install Hoolamike using Cargo
   ```bash
   cargo install --path crates/hoolamike
   ```
3. The default installation location is `~/.cargo/bin/`. Ensure the binary is in your system's `$PATH`, or reference it directly by running
   ```bash
   ~/.cargo/bin/hoolamike. 
   ```
   
If you have followed these steps correctly, You will see a help message. If you need further instruction, feel free to ask in our [Official Discord server](https://discord.gg/xYHjpKX3YP).

## Attributions

A huge shoutout to these amazing libraries that make Hoolamike possible:

- [bsa-rs](https://github.com/Ryan-rsm-McKenzie/bsa-rs): Efficient handling of BSA archives.
- [directxtex](https://github.com/Ryan-rsm-McKenzie/directxtex-rs): Processing DDS image formats with style.