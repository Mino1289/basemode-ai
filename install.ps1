<#
    base installer for Windows — downloads a release binary and hands off to `base install`.

        irm https://raw.githubusercontent.com/Mino1289/basemode-ai/main/install.ps1 | iex

    No Rust toolchain, no MSVC, no LLVM.

    Environment:
      BASE_VERSION        pin a tag, e.g. v0.14.1 (default: latest release)
      BASE_INSTALL_ARGS   passed through to `base install`

    Everything runs inside one scriptblock. Under `irm | iex` the script body would
    otherwise execute in the caller's own session, leaving $ErrorActionPreference,
    the helper functions and every temp variable behind after the install, and an
    `exit` would close the user's shell. The scriptblock scopes all of it, and
    failures throw rather than exit.
#>

& {
    $ErrorActionPreference = 'Stop'

    $repo = 'Mino1289/basemode-ai'

    function Die($msg) { throw $msg }
    function Say($msg) { Write-Host $msg }

    try {
        # ── which build ───────────────────────────────────────────────────────
        $arch = $env:PROCESSOR_ARCHITECTURE
        if ($arch -ne 'AMD64') {
            Die "unsupported architecture: $arch. Only windows-x86_64 is published; build from source (see the README)."
        }
        $asset = 'base-windows-x86_64.zip'

        if ($env:BASE_VERSION) {
            $base  = "https://github.com/$repo/releases/download/$($env:BASE_VERSION)"
            $label = $env:BASE_VERSION
        } else {
            $base  = "https://github.com/$repo/releases/latest/download"
            $label = 'latest'
        }
        $url = "$base/$asset"

        # BASE_HOME is base's own override and the only one that isolates on
        # Windows: src/home.rs notes that `dirs` never consults $HOME here, so a
        # USERPROFILE-based fake home silently targets the real tier. Honour it so
        # this script's own checks read the same home the binary writes to.
        $binHome = if ($env:BASE_HOME) { $env:BASE_HOME } else { $HOME }
        $binDir  = Join-Path $binHome '.local\bin'
        $plain   = Join-Path $binDir 'base'
        $withExe = Join-Path $binDir 'base.exe'

        function HashOf($p) {
            if (Test-Path $p) { (Get-FileHash $p -Algorithm SHA256).Hash } else { $null }
        }
        $exeBefore = HashOf $withExe

        # ── download, unpack, hand off ────────────────────────────────────────
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("base-install-" + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp -Force | Out-Null

        try {
            Say "base: fetching $asset ($label)"
            $zip = Join-Path $tmp $asset
            try {
                $prev = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'
                Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
                $ProgressPreference = $prev
            } catch {
                Die "download failed: $url"
            }

            # The binary verifies its own updates, but this first copy arrives
            # before any of that code is running. Check it here too: a release
            # publishes SHA256SUMS, and a download that does not match it is not
            # installed. (The minisign signature over SHA256SUMS is checked by
            # the binary on every later update; PowerShell cannot verify it.)
            Say "base: verifying $asset"
            $sumsPath = Join-Path $tmp 'SHA256SUMS'
            try {
                $prev = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'
                Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sumsPath -UseBasicParsing
                $ProgressPreference = $prev
            } catch {
                Die "could not download SHA256SUMS - refusing to install a download that cannot be verified"
            }

            $line = Get-Content $sumsPath |
                Where-Object { ($_ -split '\s+')[1] -eq $asset } |
                Select-Object -First 1
            if (-not $line) { Die "SHA256SUMS has no entry for $asset - refusing to install" }

            $expected = ($line -split '\s+')[0]
            $actual   = (Get-FileHash $zip -Algorithm SHA256).Hash
            if ($actual -ne $expected) {
                Die "checksum mismatch for $asset - the download is damaged or was tampered with; nothing was installed"
            }
            Say 'base: checksum verified'

            try {
                Expand-Archive -Path $zip -DestinationPath $tmp -Force
            } catch {
                Die "downloaded file is not a valid archive - check that $label exists"
            }

            $exe = Join-Path $tmp 'base.exe'
            if (-not (Test-Path $exe)) { Die 'archive did not contain base.exe' }

            Say 'base: installing'
            $installArgs = @('install')
            if ($env:BASE_INSTALL_ARGS) { $installArgs += ($env:BASE_INSTALL_ARGS -split '\s+') }

            # Run from inside the unpacked archive: `base install` locates
            # scripts\ast by trying two paths relative to the binary and then the
            # working directory, and a release archive satisfies only the third.
            Push-Location $tmp
            $prevEap = $ErrorActionPreference
            try {
                # Windows PowerShell 5.1 promotes a native command's stderr to a
                # TERMINATING NativeCommandError when $ErrorActionPreference is
                # Stop and stderr is redirected — at exit 0, on output that is
                # not an error at all. `base install` writes progress there, so
                # under 5.1 a perfectly good install aborts, and since the catch
                # now rethrows it aborts loudly. pwsh 7 does not do this, which
                # is why a pwsh-only harness cannot see it (#96).
                #
                # A native command reports failure through $LASTEXITCODE, never
                # the error stream, so Stop buys nothing here. Drop it for the
                # call and read the exit code ourselves.
                $ErrorActionPreference = 'Continue'
                & $exe @installArgs
                $rc = $LASTEXITCODE
            } finally {
                $ErrorActionPreference = $prevEap
                Pop-Location
            }
            if ($rc -ne 0) { Die "base install exited $rc" }
        }
        finally {
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }

        # ── the .exe Windows needs, refreshed on every run ────────────────────
        # `base install` writes ~\.local\bin\base with no extension, and PATHEXT
        # means Windows cannot run that by name. Copy unconditionally with -Force:
        # copying only when base.exe is ABSENT leaves an existing base.exe stale
        # on every upgrade, so the user keeps running the old binary while the
        # installer reports success — a silent no-op update, which is worse than
        # the missing-file bug this copy exists to fix.
        if (Test-Path $plain) {
            Copy-Item $plain $withExe -Force
            $exeAfter = HashOf $withExe
            if ($exeBefore -and $exeBefore -eq $exeAfter) {
                Say 'base: base.exe already current'
            } elseif ($exeBefore) {
                Say 'base: refreshed base.exe (Windows needs the extension to run it by name)'
            } else {
                Say 'base: wrote base.exe alongside base (Windows needs the extension to run it by name)'
            }
        }

        # ── PATH check, after the fact so it never blocks the install ─────────
        $onPath = ($env:PATH -split ';') -contains $binDir
        if (-not $onPath) {
            Say ''
            Say "base: $binDir is not on your PATH. Add it for your user:"
            Say ''
            Say "    `$p = [Environment]::GetEnvironmentVariable('PATH','User')"
            Say "    [Environment]::SetEnvironmentVariable('PATH', '$binDir;' + `$p, 'User')"
            Say ''
            Say '  then reopen your terminal.'
        }

        # Since #93 `base install` wires the hooks even when Claude Code is not
        # installed yet: it creates ~\.claude itself for the bundled skill, and
        # the wiring is deferred to after that rather than dropped, so a later
        # Claude Code finds them. This file therefore exists after every install
        # that did not pass --skip-hooks; if it does not, the wiring FAILED
        # rather than being deferred. Test settings.json, not the directory,
        # which `base install` creates either way.
        if (-not (Test-Path (Join-Path $binHome '.claude\settings.json'))) {
            Say ''
            Say 'base: the hooks were not wired, so base will not do anything yet.'
            Say '  Re-run the line below; if it happens twice, please report it at'
            Say '  https://github.com/Mino1289/basemode-ai/issues'
            Say ''
            Say '    base install'
            Say ''
        }
    }
    catch {
        Write-Host ''
        Write-Host "base install failed: $($_.Exception.Message)"
        Write-Host 'Nothing was changed on your PATH. Report at https://github.com/Mino1289/basemode-ai/issues'
        # Re-raise. Printing and returning would make a failed install
        # indistinguishable from a successful one to anything that checks a
        # status: CI, a wrapper script, or a user chaining `&&`. `throw` is the
        # right primitive rather than `exit 1` — under `irm | iex` the body runs
        # in the caller's own session, where `exit` closes their shell.
        throw
    }
}
