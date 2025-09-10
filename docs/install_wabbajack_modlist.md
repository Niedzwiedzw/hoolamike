# Install a mod list from Wabbajack
> [!WARNING]
> Due to NexusMods TOS, To use this feature you must have [NexusMods Premium Membership](https://next.nexusmods.com/premium). Sorry!
> 
> If you do not have NexusMods Premium membership, [consider using Wabbajack in Proton](https://github.com/Omni-guides/Wabbajack-Modlist-Linux).

This in-depth tutorial will teach you how to use Hoolamike to install Wabbajack modlists. In this example, we will be installing the popular [Viva New Vegas](https://vivanewvegas.moddinglinked.com) modlist for Fallout: New Vegas, however, the principles outlined in this guide apply to all other games. 

### Downloading from CDN
> [!NOTE]
> Downloading .wabbajack files in this manner is more taxing on their servers. Consider [donating](https://www.patreon.com/user?u=11907933) to the project for the trouble. For more information about Wabbajack, read the associated Wiki article

Wabbajack has it's own file format, the `.wabbajack` file. To install a Wabbajack modlist, you will need to download these files from their CDN manually. This fairly simple process goes as follows.

1. Click [here](https://build.wabbajack.org/authored_files) to go to the Wabbajack CDN
2. Search for the mod list you wish to install, in this case, Viva New Vegas.
3. Click the "Uploaded At" column until it shows you a down arrow. This means the list is sorted by **most recently uploaded** first.
4. Click "Slow Link (debug only)". Take note of where you intend to save this file.

### Setting configuration options
**See more: [Edit configuration file](/docs/editing_configuration_file.md#installation)**

```yaml
  wabbajack_file_path: ~/Downloads/Viva\ New\ Vegas.wabbajack
  installation_path: ~/Games/VNV
```
To prepare your configuration file, do the following

1. Open your configuration file in a text editor of your choosing.
2. Type or paste in the location of the downloaded `.wabbajack` file as shown. 
3. Set the `installation_path` to where you would like your mod pack to be installed.
4. Save your configuration file.

### Starting installation

Finally, save any unfinished work you may have on your computer and close any programs you have running in the background.
 
Next, open a terminal window and type in the following command.

> ```bash
> ulimit -n 64556 && hoolamike install
> ```

The length of the installation process is proportionate to the size of the mod list you are installing, so it is best to leave your computer while the process is ongoing.

Once the installation process is complete, the mod list will be installed at the `installation_path` you have specified. 

### Finishing up

Now that you have finished this tutorial, you may add the downloaded Mod Organizer instance to Steam. For more information about Mod Organizer 2 on Linux, [please read Omni's extremely comprehensive tutorial](https://github.com/Omni-guides/Wabbajack-Modlist-Linux/wiki/General-Linux-Guide-(Anvil)#step-2---add-modorganizerexe-as-a-non-steam-game).