![**Hoolamike**](./media/hoolamike-logo.png)

# 🏄 Hoolamike: Wabbajack Modlist Installer for Linux 

Hoolamike is here to ease the process of installing **Wabbajack modlists** on Linux without the hassle of virtual machines or Wine. This project ports the functionality on a **per-modlist basis**, enabling seamless integration with your favorite games. 🌟
## 🎮 Games currently confirmed to work
- [**Stardew Valley**](https://store.steampowered.com/app/413150/Stardew_Valley/) 🥕  
- [**Skyrim**](https://store.steampowered.com/app/489830/The_Elder_Scrolls_V_Skyrim_Special_Edition/) 🐉 
- [**Fallout 4**](https://store.steampowered.com/app/377160/Fallout_4/) ☢️  
- [**Fallout: New Vegas**](https://store.steampowered.com/app/22380/Fallout_New_Vegas/) ☢️  
- [**Fallout 3**](https://store.steampowered.com/app/22300/Fallout_3/) ☢️  

## 🚀 Lists currently confirmed to work (all lists for above games SHOULD work, but we cannot test them all without your help)
- [**Tuxborn**](https://github.com/Omni-guides/Tuxborn): A Wabbajack Modlist for Skyrim SE designed with the Steam Deck in mind 🐉  
- [**Wasteland Reborn**](https://github.com/Camora0/Wasteland-Reborn): modlist that aims to create a fully featured, lore friendly rebuild of Fallout 4 from the ground up.  ☢️  
- [**Magnum Opus**](https://github.com/LivelyDismay/magnum-opus): what one would describe as a "kitchen sink list." ☢️  
- [**Begin Again**](https://www.nexusmods.com/newvegas/mods/79547): modlist focused on bringing the gameplay feel of later Fallout titles into Fallout 3 and New Vegas ☢️  

## 🔮 Wishlist & Community Support

Want a specific modlist to be supported? 🤔  
Join our **[Discord Community](https://discord.gg/xYHjpKX3YP)** and let us know! Your feedback and suggestions drive this project forward. 💬

## 💡 Goals

Wabbajack modlist installation logic is being slowly ported from C# to Rust, while keeping linux support constantly in mind.  🛠️
## ❌ Non-goals 
Replacing Wabbajack, modlist creation - this project focuses only on installation of modlists keeping them compatible with Wabbajack.

## 🙏 Special Thanks

A huge shoutout to these amazing libraries that make Hoolamike possible:

- [bsa-rs](https://github.com/Ryan-rsm-McKenzie/bsa-rs) 🗂️: Efficient handling of BSA archives.
- [directxtex](https://github.com/Ryan-rsm-McKenzie/directxtex-rs) 🖼️: Processing DDS image formats with style.

Make sure to ⭐ star their repositories — their work is the backbone of this project!

## ⭐ Features

- 🐧 **Linux Native**: Say goodbye to VMs or Wine setup, enjoy quick and streamlined installation process.  
- ⚡ **Optimized for Performance**: Parallelization of installation steps is one of the main focuses. As much as possible is performed in a multithreaded fashion to lower the wait time.
- 📚 **Per-Modlist Focus**: Tailored support for specific modlists.  

## 🏄 How to Get Started with Hoolamike
1. Download the latest release from https://github.com/Niedzwiedzw/hoolamike/releases, unpack the archive and give appropriate permissions to hoolamike (`chmod u+x hoolamike`). You can place the binary wherever you want.
2. Configure Hoolamike:
    Run `hoolamike print-default-config > hoolamike.yaml` to generate a default configuration file. (or ask for examples on **[Discord Community](https://discord.gg/xYHjpKX3YP)**)
3. Edit `hoolamike.yaml` in a text editor. Add your Nexus API key, which you can obtain from https://next.nexusmods.com/settings/api-keys.
    Specify game directories, such as:
```
  games:
    Fallout4:
      root_directory: "/path/to/Fallout 4/"
```
4. Obtain the required modlist file: Download the <modlist-name>.wabbajack file for your desired modlist. You might need to check the Wabbajack community for the appropriate link. Place this file in the same directory as hoolamike.yaml.
5. Update the configuration: In `hoolamike.yaml`, set the path to the downloaded .wabbajack file under `installation.wabbajack_file_path`.
6. Install the modlist: Run `hoolamike install`. 

If you face any issues, consult the **[Discord Community](https://discord.gg/xYHjpKX3YP)** for further guidance or file a support ticket.

## 🚧 Compiling from source
1. Install the Rust toolchain: Visit https://rustup.rs/ to install Rust.
2. Clone the Hoolamike repository: Run git clone https://github.com/Niedzwiedzw/hoolamike to download the project files.
3. Install Hoolamike using Cargo: Navigate to the repository and execute `cargo install --path crates/hoolamike`.
4. Verify the installation: Once installed, the binary will typically be located in ~/.cargo/bin/. Ensure the binary is in your system's $PATH, or reference it directly by running ~/.cargo/bin/hoolamike. You should see a help message indicating successful installation.## 💬 Join the Community

Whether you're here to wishlist modlists, contribute, or just chat with fellow enthusiasts, our **[Discord Community](https://discord.gg/xYHjpKX3YP)** is open for you! 🎉

## 🌟 Contributing

We welcome contributions of all kinds! Whether it’s fixing bugs, improving documentation, or adding support for new modlists, your help is appreciated. Check out our `CONTRIBUTING.md` for guidelines.

---

Hoolamike is built **by Linux gamers, for Linux gamers**. Let's make modding on Linux better together! 🐧✨
