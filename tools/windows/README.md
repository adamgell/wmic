# Development guest execution

The selected reference guest is the Parallels VM `{ec3bc428-7a31-4f17-8424-7ce78923f7f3}`. Parallels Tools executes as SYSTEM; interactive-user behavior needs a separate console test. The existing `\\Mac\Home` share is readable in this guest. It is a development transfer path, not an application dependency.

`stage-artifact.ps1 -SourcePath <file> -ExpectedSha256 <hash>` verifies both source and staged bytes and places the executable in `C:\ProgramData\WmicCompat\artifacts\<hash>\wmic.exe`. It never replaces the Microsoft executable. `-FileName backend-contract.exe` can stage the ignored native adapter test binary.
