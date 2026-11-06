# Installs the dagon command from a GitHub release, for Windows.
#
#     irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.ps1 | iex
#
# A version:      $env:DAGON_VERSION = "1.2.3"    (default: the latest)
# The folder:     $env:DAGON_INSTALL_DIR          (default: %LOCALAPPDATA%\Programs\dagon)
# Leave PATH as:  $env:DAGON_NO_MODIFY_PATH = "1"
#
# It stops on any download or checksum error and leaves the folder as it was.
# It needs no administrator rights. It runs on Windows PowerShell 5.1.
#
# Everything is inside Install-Dagon, called on the last line, so a download
# cut short runs nothing. Errors are thrown, never `exit`, so the window stays.

function Install-Dagon {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    $repo = 'c-ardinal/DAGON-Boot'

    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
        throw 'dagon install: this script is for Windows; use install.sh'
    }
    $machine = $env:PROCESSOR_ARCHITEW6432
    if (-not $machine) { $machine = $env:PROCESSOR_ARCHITECTURE }
    if ($machine -ne 'AMD64') {
        throw "dagon install: no release for Windows $machine; see https://github.com/$repo#readme to build it yourself"
    }
    $target = 'x86_64-pc-windows-msvc'

    $version = $env:DAGON_VERSION
    if (-not $version) {
        # Every component has its own releases: only cli-v<version> counts, and
        # a pre-release does not. The list comes newest first.
        try {
            $list = Invoke-WebRequest -UseBasicParsing -Uri "https://api.github.com/repos/$repo/releases?per_page=100"
        } catch {
            throw 'dagon install: cannot read the list of releases; name a version with $env:DAGON_VERSION'
        }
        $found = [regex]::Match([string]$list.Content, '"tag_name":\s*"cli-v(\d+\.\d+\.\d+)"')
        if (-not $found.Success) { throw 'dagon install: no release of the dagon command found' }
        $version = $found.Groups[1].Value
    }
    if ($version -notmatch '^[0-9][0-9A-Za-z.-]*$') { throw "dagon install: not a version: $version" }

    $dir = $env:DAGON_INSTALL_DIR
    if (-not $dir) { $dir = Join-Path $env:LOCALAPPDATA 'Programs\dagon' }
    $archive = "dagon-cli-$version-$target.zip"
    $url = "https://github.com/$repo/releases/download/cli-v$version/$archive"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $file = Join-Path $tmp $archive
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $file
            Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256" -OutFile "$file.sha256"
        } catch {
            throw "dagon install: cannot download $url"
        }

        $want = ([string](Get-Content -Raw -LiteralPath "$file.sha256")).Trim() -split '\s+' | Select-Object -First 1
        if ($want -notmatch '^[0-9a-fA-F]{64}$') { throw "dagon install: $archive.sha256 holds no checksum" }
        $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash
        if ($want -ine $got) { throw "dagon install: $archive does not match its checksum; nothing was installed" }

        $unpacked = Join-Path $tmp 'unpacked'
        Expand-Archive -LiteralPath $file -DestinationPath $unpacked
        $exe = Join-Path $unpacked 'dagon.exe'
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "dagon install: $archive holds no dagon.exe" }

        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Item -LiteralPath $exe -Destination (Join-Path $dir 'dagon.exe') -Force
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Output "dagon $version ($target) installed in $dir"

    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $onPath = @($user -split ';') -contains $dir
    if ($onPath) { return }
    if ($env:DAGON_NO_MODIFY_PATH) {
        Write-Output "$dir is not on your PATH. Add it in Settings, under your user's environment variables."
        return
    }
    $new = if ($user) { "$user;$dir" } else { $dir }
    [Environment]::SetEnvironmentVariable('Path', $new, 'User')
    $env:Path = "$env:Path;$dir"
    Write-Output "$dir added to your user's PATH. A new window finds dagon too."
}

Install-Dagon
