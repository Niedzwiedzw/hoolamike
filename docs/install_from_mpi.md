# Install mods from an .MPI file.
>![IMPORTANT]
> This tutorial is redundant for Windows users, as you may instead use the reference installer included with all `.mpi` mods.

Some mods for Fallout: New Vegas are packaged in the `.mpi` format. For an in-depth explanation of how an .mpi file is packaged, you may read [the official specification](https://docs.google.com/document/d/1yIGmtE69ZOCJEJw8eFdVDSPxDf7TFPA9Hjq2bcbM9yU). 

#### Popular mods packaged as MPI:
- [Tale of Two Wastelands](https://mod.pub/ttw/133-tale-of-two-wastelands)
- [MAC-TEN FO3 TTW](https://www.nexusmods.com/newvegas/mods/90284)
- [Ultimate Edition ESM Fixes Remastered](https://www.nexusmods.com/newvegas/mods/92289)
- [FNV BSA Decompressor](https://www.nexusmods.com/newvegas/mods/65854)

This guide will show you how to install Tale of Two Wastelands as an example. You may use the same principles to install any other mod packaged as `.mpi`.

### Preparation

0. Before you begin, you will need to download the mod you wish to install. In this case, we will assume you are downloading `Tale of Two Wastelands`.

1. When the download is complete, extract the archive and open it's contents. The contents of the Tale of Two Wastelands archive are laid out like so:
    ```txt
    TTW Install Without OGG Reencoding.bat
    TTW Install.exe
    Tale of Two Wastelands 3.4.mpi
    bass.dll
    bassenc.dll
    bassenc_mp3.dll
    bassenc_ogg.dll
    bassmix.dll
    oggenc2.exe
    xdelta3.dll
    ```

Note down the location of the **`Tale of Two Wastelands.mpi`** file on your computer. You will need this for the next step. Everything else in this folder **can safely be ignored**, as these are resources for the Windows installer.

### Configuration

```yaml
extras:
  tale_of_two_wastelands:
    path_to_ttw_mpi_file: "./ttw.mpi"
    variables:
      USERPROFILE: "$FNVCONFIGDIR"
      DESTINATION: "./out"
```

### Installation

Once you have finished editing your configuration file, save any unfinished work you may have on your computer and close any programs you have running in the background. 
 
Next, open a terminal window and type in the following command.

> ```bash
> ulimit -n 64556 && hoolamike install
> ```

The installation process is quite lengthy and quite intensive, so it is best to leave your computer while the process is ongoing.

Once the installation is complete, your mod files will be located at the `DESTINATION` you have specified. 

You can then import this folder into your mod manager of choice.

### Wrapping up

If you wish to install multiple MPI mods (i.e. you are following any ModdingLinked guide), you will need to repeat these instructions in the order you are asked to install them from the mod guide.

---
Here is the complete demonstration `.yaml` file, for you to adapt for your own purposes.

```yaml
games:
  FalloutNewVegas:
    root_directory: ~/.local/share/Steam/steamapps/Fallout\ New\ Vegas
  Fallout3:
    root_directory: ~/.local/share/Steam/steamapps/Fallout\ 3\ goty
fixup:
  game_resolution: 1920x1080

extras:
  tale_of_two_wastelands:
    path_to_ttw_mpi_file: "./ttw.mpi"
    variables:
      USERPROFILE: "$FNVCONFIGDIR"
      DESTINATION: "./out"
```