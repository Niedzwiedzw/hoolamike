# Changelog

All notable changes to this project will be documented in this file.

## [unreleased]

### 🚀 Features

- *(gui)* Some help messages, allow resize, small layout fixes

### 🐛 Bug Fixes

- *(gui)* Fix file saving

## [0.19.0] - 2025-08-23

### 🚀 Features

- *(gui)* Hoolamike gui ALPHA (config builder for now)

## [0.18.4] - 2025-08-15

### 💼 Other

- *(progress)* Updated the progress bar library to newer version - should eliminate the occasional PoisonError panics

## [0.18.3] - 2025-08-15

### 💼 Other

- *(compression)* Fix paths being too long while keeping some of the context

## [0.18.2] - 2025-08-14

### 🚀 Features

- *(textured)* Use texconv.exe wrapped in wine for gpu-accellerated texture recompression

## [0.17.0] - 2025-08-09

### 🚀 Features

- *(7zip)* Single-threaded execution of extraction to prevent collision with app multithreaded executor

### 📚 Documentation

- *(readme)* Add notes about ulimit problem

## [0.16.1] - 2025-07-16

### 🚀 Features

- *(intel_tex)* Make intel tex optional

## [0.16.0] - 2025-07-16

### 🚀 Features

- *(wabbajack-file)* More permissive top level file parsing (will work with extra fields, missing non-important fields)
- *(hoolamike)* Remove misleading validate-modlist subcommand

### ⚙️ Miscellaneous Tasks

- *(textures)* Disable intel tex

## [0.15.7] - 2025-05-12

### ⚙️ Miscellaneous Tasks

- *(ci)* Update ubuntu image

## [0.15.6] - 2025-05-12

### 🚀 Features

- *(archive)* Another 7zip implementation to try before 7zip cli, intel tex decompression library integration for SIMD accelerated BC7 handling

## [0.15.5] - 2025-03-18

### 🐛 Bug Fixes

- *(7zip)* Make 7zip filenames case-insensitive when extracting

## [0.15.3] - 2025-03-12

### 🐛 Bug Fixes

- *(windows)* Fix windows build, remove unnecessary compiler feature flags

## [0.15.2] - 2025-03-12

### 🐛 Bug Fixes

- *(nexus)* Fix linux desktop entry for automatic nxm handling, better error messages

## [0.15.1] - 2025-03-11

### 🐛 Bug Fixes

- *(nexus)* Go directly to the specific file download link for multifile mods

## [0.15.0] - 2025-03-11

### 🚀 Features

- *(nexus)* Support nxm link handling, allowing downloads for non-premium accounts

## [0.14.1] - 2025-02-22

### 🐛 Bug Fixes

- Unused imports

### ⚙️ Miscellaneous Tasks

- Update octadiff reader to compile on latest nightly rust

## [0.14.0] - 2025-01-24

### 🚀 Features

- *(fnv)* Fallout new vegas 4gb patcher functionality is now built into hoolamike (no need to run FNVPatch.exe or anything like that)

## [0.13.0] - 2025-01-23

### 🚀 Features

- *(bsa)* Unpacking multiple bsa files is now significantly faster, audio cli
- *(ttw)* Fixed resampling at cost of higher memory usage
- *(ttw)* Use smallvec to speed up allocations

### 🐛 Bug Fixes

- *(ttw)* Respect compression requirement

### ⚙️ Miscellaneous Tasks

- Performance flags by default

## [0.12.5] - 2025-01-21

### 🚀 Features

- *(fnv)* Bsas are now compressed using correct format (fixes Begin Again and Tale of two Wastelands)

## [0.12.4] - 2025-01-20

### 🚀 Features

- *(ttw)* Fix the last-modified timestamps for files

## [0.12.3] - 2025-01-19

### 🚀 Features

- *(ttw)* Implement the CLI post-fixup-command functionality. bonus is that we don't execute arbitrary shell commands - input is parsed and validated

## [0.12.2] - 2025-01-19

### 🚀 Features

- *(ttw)* Optimize performance by easing down on logging a little bit and splitting the operations into chunks to prevent flooding user drive with temporary files

### 🚜 Refactor

- *(ttw)* Split into modules and cleanup

## [0.12.1] - 2025-01-19

### 🐛 Bug Fixes

- *(ttw)* Archives in bsa are case insensitive, and ttw installer makes extensive use of it

## [0.12.0] - 2025-01-18

### 🚀 Features

- *(hoola-audio)* Mp3 handling
- *(hoola-audio)* Ogg and wav support
- *(hoola-audio)* Better logging
- *(ttw)* Stage I
- *(ttw)* Stage II
- *(ttw)* Stage III
- *(ttw)* Stage IV
- *(ttw)* Stage V
- *(ttw)* Stage VI
- *(ttw)* Stage VII
- *(ttw)* Stage VIII (variables)
- *(ttw)* Asset::Copy
- *(ttw)* Stage IX (all assets initially handled)
- *(ttw)* Multithreading support
- *(ttw)* Ttw installer functionality is fully ported ☢️

### 🐛 Bug Fixes

- *(ttw)* Variable resolving
- *(ttw)* Ogg resampling
- *(ttw)* Fix asset handling

### 🚜 Refactor

- *(ttw)* Type safety for manifest file
- Cleanup warnings
- Cleanup ttw installer code

## [0.11.3] - 2025-01-15

### 🚀 Features

- *(archives)* Bethesda archives now extract a bit more optimally, extracting archives through cli

## [0.11.2] - 2025-01-14

### 🐛 Bug Fixes

- *(windows)* Switch to platform-agnostic file size reading
- *(archives)* 7z extraction for windows-encoded paths no longer fails on linux

### 📚 Documentation

- *(readme)* Update the installation instructions
- *(readme)* Add the remaining supported games to readme
- *(readme)* Fix emoji
- *(readme)* Notes about support

## [0.11.0] - 2025-01-08

### 🚀 Features

- *(modlist-file)* Modlist file is preloaded at start, sacrificing some disk space but speeding up applying binary patches
- *(archives)* Preheat archives in chunks of 30GB so that no more than (hopefully) that is taken up by hoolamike while installing
- *(archives)* Preheat archives in chunks, but also prioritize things other than 7z which is absurdly slow

### 🐛 Bug Fixes

- Point to hosted version of indicatif fork
- Drop file handles for preextracted wabbajack files
- Limit max open files when extracting wabbajack file

### 🚜 Refactor

- Refactor archive preloading logic
- Refactor nested archive directives

## [0.10.0] - 2025-01-07

### 🚀 Features

- *(modlist-file)* Added more definitions (Mega downloader and BSAs with 32 bit FileFlags - which is very weird and should be investigated)
- *(modlist-file)* Load modlist file in one go instead of buffering it - faster

## [0.9.2] - 2025-01-06

### 🚀 Features

- *(deps)* Remove openssl dependency to enable working on steam deck

## [0.9.1] - 2025-01-06

### 🚀 Features

- *(archives)* More readable error messages for archive extraction failures
- *(textures)* Recompressing textures using BC7 methods now uses the minimal level, which results in 100x decrease in speed

### 🐛 Bug Fixes

- Detect lzma method 14 archives to offload them to 7z binary
- *(install)* Fix paths dropping to early
- *(archives)* Check windows encoding and normalize paths when using CompressTools
- *(installed)* ModOrganizer.ini and other remapped files will no longer be places at paths relative to CWD

## [0.8.11] - 2025-01-05

### 🚀 Features

- *(installer)* Hoolamike will now mimic the windows case-insensitive path lookup in case copying a local file (typically game directory) fails

## [0.8.10] - 2025-01-05

### ⚙️ Miscellaneous Tasks

- Cache and write permissions

## [0.8.9] - 2025-01-05

### ⚙️ Miscellaneous Tasks

- Cache

## [0.8.6] - 2025-01-05

### ⚙️ Miscellaneous Tasks

- Use secrets for tokens

## [0.8.5] - 2025-01-05

### ⚙️ Miscellaneous Tasks

- Only run CI on new versions

## [0.8.2] - 2025-01-05

### 🚀 Features

- *(ci)* CI with automatic publishes

### 💼 Other

- Getting started

<!-- generated by git-cliff -->
