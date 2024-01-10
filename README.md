NativeStart
====

A modern alternative to WebStart.

Distribute your JVM desktop app by providing a small executable that will download the JVM and the application and keep both up-to-date automatically. So simple!

### Features
- Single, small executable (no java process visible)
- No prerequisites for the users (no JVM, no WebStart)
- Automatic application and JVM download and updates built-in by design
- JSON based application descriptor
- DSL for splash screens
- SHA-256 or BLAKE-3 digests to detect modifications on installed files or pending updates
- Optional Ed25519 key integrated in executable. Only correctly signed application descriptors will be started.

### Splash DSL

The file format contains 3 parts: Information about the size of the splash window, commands for drawing the background and commands for drawing the progress.
````
splash <w> <h>

[background]
<commands>

[progress]
<commands>
````
The commands have parameters, which can use arithmetic expressions and variables in the form `${var}`. The following variables are supported:
- `dpi`: The DPI mode of the screen
  - contains `mdpi` if screen zoom factor is smaller than 1.25. Coordinates get multiplied by 1.0
  - contains `hdpi` if screen zoom factor is between 1.25 and 1.75 (exclusive). Coordinates get multiplied by 1.5
  - contains `xhdpi` if screen zoom factor is grater than 1.75. Coordinates get multiplied by 2.0
- `version`: The version of the application as defined int the JSON descriptor
- `progress`: The download progress as value between 0 and 1

Commands:
- `image <path> <x> <y> [<w> <h>]` Draw image at given position (width and height are optional)
- `textfont <path>` Use the font stored in the given file (TTF, OTF, etc.)
- `textsize <size>` Use the given font size
- `textalign <start|left|end|right|center>` Use the given font alignment
- `fill <r> <g> <b>` Set the fill color (RGB)
- `filltext <x> <y> <text>` Write the given text at the given position

Example:
````
splash 512 300

[background]
image splash_${dpi}.png 0 0
textfont myfont-min.ttf
textsize 18
fill 0 0 0
textalign start
filltext 395 110 ${version}

[progress]
image splash_progress_border_${dpi}.png 0 0
image splash_progress_filled_${dpi}.png 0 0 6+${progress}*500 300-6
````

All resources (images and fonts) and the descriptor (a file called `splash`) need to be packed as tar.xz archive.

### Hiding Splash
By default, the splash screen gets hidden, when the Java application starts. For applications that need some time until the UI is ready, it is possible to add a static method `public static void awaitUI()` in the same class as the `main` method. If it exists, this method is called by NativeStart shortly after the `main` method and the splash screen only gets hidden once this method returns.

This repository...
---
... contains the native application downloading the JVM and the application and starting it. In addition, it shows a splash screen until the application is ready.

### How to build
- Build generic executable to be customized by nativestart-packer
  - for unsigned applications: `cargo build --release --bin generic`  
  - for signed applications: `cargo build --release --bin generic --features check-signature`  