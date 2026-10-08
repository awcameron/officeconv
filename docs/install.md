# Installing

The [README](../README.md#install) has the steps most people need: download a binary and put it
on your `PATH`, or build it with Cargo. This page covers the rest.

- [Which Linux build](#which-linux-build)
- [Downloading with the GitHub CLI](#downloading-with-the-github-cli)
- [Checking where a binary came from](#checking-where-a-binary-came-from)
- [Unsigned binaries](#unsigned-binaries)
- [An older version](#an-older-version)
- [Building from source](#building-from-source)
- [Uninstall](#uninstall)

## Which Linux build

The `gnu` Linux builds need glibc 2.17 or later, which nearly every distribution has. The `musl`
builds are statically linked and need no system libraries, so they also run on Alpine and in
minimal container images such as `scratch` or distroless.

For PDF output with characters Noto Sans lacks, the system still needs a font that has them; see
[PDF](formats.md#pdf).

## Downloading with the GitHub CLI

The [GitHub CLI](https://cli.github.com) can download the same two files in place of the `curl`
line in the [README's steps](../README.md#macos-and-linux):

```sh
gh release download -R awcameron/officeconv -p "officeconv-$target.*"
```

## Checking where a binary came from

The checksum shows the download wasn't damaged, but it sits on the same page as the archive. To
check that the archive was built by this repository's release workflow, from a commit in it, use
the GitHub CLI:

```sh
gh attestation verify "officeconv-$target.tar.gz" -R awcameron/officeconv
```

It prints the workflow and commit that built the file, and fails for anything else. On Windows,
name the `.zip` instead.

## Unsigned binaries

The binaries aren't code-signed by Apple or Microsoft. A file downloaded in a browser is marked
as coming from the internet, so macOS refuses to open it and Windows SmartScreen warns about it;
`curl`, `gh` and `Invoke-WebRequest` downloads aren't marked. On macOS,
`xattr -d com.apple.quarantine officeconv` removes the mark.

## An older version

To get one version instead of the latest, replace `latest/download` with `download/<tag>` in the
URL, such as `download/v0.3.8`. Older releases differ in three ways:

- before v0.3.5, file names include the version, such as `officeconv-v0.3.4-<target>.tar.gz`;
- before v0.3.6, releases aren't attested;
- before v0.3.7, there are no `musl` or Windows arm64 builds.

## Building from source

The [README](../README.md#build-from-source) has the command to build from the repository on
GitHub. From a clone of it, run `cargo install --locked --path .` instead. Either one puts
`officeconv` in `~/.cargo/bin`. `--locked` builds with the dependency versions in `Cargo.lock`,
the ones CI tests and checks for security advisories. The build takes several minutes, because the
release profile optimizes the whole program at once.

PDF output is included by default. To leave it out for a smaller binary, add
`--no-default-features`; `--to pdf` then says it isn't built in.

## Uninstall

Delete the binary (`rm ~/.local/bin/officeconv`, or the `officeconv` folder in
`%LOCALAPPDATA%\Programs` on Windows), or run `cargo uninstall officeconv` if you built it with
Cargo.
