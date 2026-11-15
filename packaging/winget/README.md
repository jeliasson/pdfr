# winget

Templates for `winget install kjellberg.pdfr`, submitted to
<https://github.com/microsoft/winget-pkgs> once Windows releases are tested.
Easiest: after a release, run

```powershell
wingetcreate new https://github.com/kjellberg/pdfr/releases/download/v0.1.0/pdfr-v0.1.0-windows-x64.msi
```

which fills in these fields from the installer. Later versions:
`wingetcreate update kjellberg.pdfr -v <version> -u <msi url> --submit`.
