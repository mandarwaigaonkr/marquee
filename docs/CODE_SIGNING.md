# Code signing

**Status: releases are not signed yet.** Windows SmartScreen therefore warns when the installer is run. Choose **More info**, then **Run anyway**.

## Check a download

Every release lists the SHA-256 of its installer. Compare it with the file you downloaded:

```powershell
Get-FileHash .\Marquee_0.1.0_x64-setup.exe -Algorithm SHA256
```

## The plan

1. Apply to [SignPath Foundation](https://signpath.org), which signs open-source projects free of charge. It needs a public repository, releases built by CI from this repository, and a published signing policy (this page becomes it).
2. Offer Marquee through `winget`.
3. Consider a Microsoft Store listing, where the Store signs the package.

A certificate does not remove the SmartScreen warning on its own: SmartScreen trusts a signed publisher once enough people have installed its releases. Signing is what lets that trust build up and carry over from one release to the next.

## Privacy

Marquee makes no network requests. It has no telemetry, no update check and no account.
