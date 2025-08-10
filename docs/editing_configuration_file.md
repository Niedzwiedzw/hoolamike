# Editing the configuration file

This document will show you every config option currently available, along with a brief explanation of what each option does. 

Let's start with a finished file.

```yaml
# This is a comment
downloaders:
  downloads_directory: ~/Downloads/
  nexus:
    api_key: +VWuwXUnzFb55qkaKucSEwagfSuwvYFUAuIR1D1R0HfGmD2PDeBT--4PnZEZQC3xC0A1BI--blyf8bDxV/n5VhCIeZKXfQ==
installation:
  wabbajack_file_path: ~/Downloads/Viva\ New\ Vegas.wabbajack
  installation_path: ~/Games/VNV
games:
  Fallout3:
    root_directory: ~/local/share/Steam/steamapps/common/Fallout\ 3\ goty
  SkyrimSpecialEdition:
    root_directory: ~/local/share/Steam/steamapps/common/Skyrim\ Special\ Edition
fixup:
  game_resolution: 1280x800
extras:
  tale_of_two_wastelands:
    path_to_ttw_mpi_file: "~/Downloads/Tale\ of\ Two\ Wastelands/Tale of Two Wastelands\ 3.4.mpi"
    variables:
      USERPROFILE: "~/.local/share/Steam/steamapps/compatdata/22380/pfx/drive_c/users/steamuser/My Documents/My\ Games/FalloutNV"
      DESTINATION: "~/"
```
If this looks overwhelming to you, don't worry. This is just an example. In all likelihood, you will not (and should not) have all of these options enabled at once. 

To disable an option, simply comment it out, you can do so by putting a `#` symbol before a line. This will mean that Hoolamike will ignore it completely.

You may have noticed back slashes before spaces. All special characters, like spaces and percent need to be 'escaped`.

You can have multiple configuration files at once. Any valid configuration file can be referenced like so
   ```bash
   hoolamike -c /path/to/config/file [OPTION]
   ```

## Downloaders
This section of the configuration file is for configuring download sources. The only supported download source right now is **NexusMods**. 

Filling out both fields is a necessary step if you wish to install mod lists with Wabbajack.

---
```yaml
downloaders:
  downloads_directory: ~/Downloads/
  nexus:
    api_key: +VWuwXUnzFb55qkaKucSEwagfSuwvYFUAuIR1D1R0HfGmD2PDeBT--4PnZEZQC3xC0A1BI--blyf8bDxV/n5VhCIeZKXfQ==
```
#### downloads_directory
The `downloads_directory` specifies where you would like Hoolamike to store downloaded mod files.

#### nexus
This is the shorthand for NexusMods.
##### nexus.api_key
The `api_key` controls Hoolamike's ability to contact NexusMods' download servers. To enable downloading from NexusMods, you will need to create a Personal API Key. To do so:

> [!WARNING]
> Never, ever, ***ever*** share your API keys with anyone. Your key give you access to your NexusMods account. If someone has access to your keys, they could get you banned... _or worse._

1. Click [here](https://next.nexusmods.com/settings/api-keys#:~:text=Request%20Api%20Key-,Personal%20API%20Key,-If%20you%20are)
2. Click "Request Api Key"
3. The generated key is automatically copied to your clipboard. Paste the copied key into your Hoolamike configuration file.

## Installation
**See more: [Install mod list from Wabbajack](/docs/install_wabbajack_modlist.md)**

```yaml
installation:
  wabbajack_file_path: ~/Downloads/Viva\ New\ Vegas.wabbajack
  installation_path: ~/Games/VNV
```

#### wabbajack_file_path
This is the file path of the Wabbajack file you intend to install.

#### installation_path
This is the file path where the output of that installation process will be stored.

## Games
This section tells Hoolamike where to look for your installed games. Game titles are in CapitalCase as shown. 
```yaml
games:
  Fallout3:
    #This is just an example, you might have your games stored elsewhere.
    root_directory: ~/local/share/Steam/steamapps/common/Fallout\ 3\ goty
  SkyrimSpecialEdition:
    root_directory: ~/local/share/Steam/steamapps/common/Skyrim\ Special\ Edition
```
#### GameName
This controls which game Hoolamike is looking for. Game names are in CapitalCase.

##### GameName.root_directory
This is where the game you wish to modify's files are stored on your computer. This will vary based on what launcher you are using (Steam, GOG, Heroic, Epic Games, etc) and what game you are intending to modify. 

If you don't know this information, that's okay, just go to your preferred search engine and look up "Where are games stored..." (e.g. Steam on Linux, GOG on macOS). 

Once you have located your game, paste it into the correct field, like the example above.

## Fixup
```yaml
fixup:
  game_resolution: 1280x800
```
#### game_resolution
This key is for Bethesda games, and sets the game resolution to a specified value in the format `WxH`. The default is `1280x800`, which is the screen resolution of the Steam Deck.

## Extra
**See more: [Installing from .mpi](/docs/install_from_mpi.md)**

```yaml
extras:
  tale_of_two_wastelands:
    path_to_ttw_mpi_file: "~/Downloads/Tale\ of\ Two\ Wastelands/Tale of Two Wastelands\ 3.4.mpi"
    variables:
      USERPROFILE: "~/.local/share/Steam/steamapps/compatdata/22380/pfx/drive_c/users/steamuser/My Documents/My\ Games/FalloutNV"
      DESTINATION: "~/"
```
#### tale_of_two_wastelands
The built-in `.mpi` handler. Primarily used for installing, as the namesake implies, popular Fallout: New Vegas mod, Tale of Two Wastelands

##### tale_of_two_wastelands.path_to_ttw_mpi_file
This key defines the path to your chosen `.mpi` file.

##### tale_of_two_wastelands.variables
This section will feed specific environment variables into your chosen `.mpi` file.