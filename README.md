# Gondola Archive Server #

This project contains the server used for serving the original Gondola Archive as seen on https://gondola.stravers.net/

# Getting Started #

1. Clone this repository: `git clone github.com/kstrafe/gondola-archive`
2. Install `cargo` and `gcc` (If using `nix`, you can use `nix develop` for this purpose).
3. Run `cargo run --release` to start the server. It will run on port 8081.

# Description of Directories #

    .
    ├── Cargo.lock         - Cargo lock file, specifies exact dependency versions used by Rust's Cargo tool
    ├── Cargo.toml         - Cargo dependency file, specifies which dependencies to utilize
    ├── files              - Files pertaining to the server
    │   ├── css            - Stylesheets are stored here
    │   ├── favicon        - Favicon images of various sizes
    │   ├── images         - Images used by the site
    │   ├── js             - Any scripts go here
    │   ├── logs           - The webserver will dump logs here, and rotate them automatically, you can `tail -f log` to see all requests live
    │   ├── misc           - robots.txt is here, other accesses will need to go through `/files/misc/X` to access misc items
    │   ├── sources        - Files matching the name of video/ files, containing plain text with the source (music source or image source)
    │   ├── statistics     - Plaintext files containing a number describing the view counts of each video. Matches the filenames in video/
    │   └── video          - All video files
    ├── flake.nix          - The `nix` dependency file, you can ignore this if you do not use nix
    ├── flake.lock         - The `nix` dependency *lock* file, you can ignore this if you do not use nix
    ├── password           - The hashed password file. Is used for `/shell` commands. Use `echo -n '<your password here>' sha512sum | cut -d ' ' -f 1 > password` to generate anew
    ├── README.md          - This file
    ├── src
    │   ├── config.rs      - Site configuration (port, disqus settings)
    │   ├── main.rs        - Main source code of the site
    │   └── writer.rs      - Wrapper around FileRotate (for log rotation)
    └── target             - Generated directory by Cargo
        └── ...

# Useful Info #

The site has a 30 minute interval in updating the `/list` page. Every time it updates, it will do the following:

1. Dump the view count values from memory to `files/statistics/` for each video
2. Read `files/video/` and from that generate a new `/list` page, this also reads from `files/sources/` to update the source information

When administering the site, all you need to do is put files in `files/video/`, and within 30 minutes the video will be listed on the `/list`, and will available to autoplay.
When removing a video, all you need to do is remove the file from `files/video/`, and it will be removed from the list within 30 minutes.

## Variables to Change ##

You will need to change some configuration variables to get disqus integration working for your site.
You'll need to change values in `src/config.rs` and run `cargo run --release` again for these changes to take effect.

## Shell ##

Accessing /shell on the site gives you a little command line. This command line allows you to change the announcement bar at the top of the site (except in /list), and is useful for announcing changes whilst the site is running. The password field uses the password file in order to authorize the command to be run. The password you type in is sha512 hashed and compared to the password file on server. Use `echo -n '<your password here>' sha512sum | cut -d ' ' -f 1 > password` in bash to create a new password file.

## December ##

In December the favicon changes where Gondola has a little christmas hat.
