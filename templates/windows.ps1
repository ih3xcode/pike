$ErrorActionPreference = "Stop"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "This script must be run as Administrator."
    exit 1
}

# Check if already installed
if (Get-Service -Name "CSFalconService" -ErrorAction SilentlyContinue) {
    Write-Host "CrowdStrike Falcon sensor is already installed. Skipping."
    exit 0
}

$tmpDir = Join-Path $env:TEMP ("pike_" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
    $hostName = $env:COMPUTERNAME
    $arch = $env:PROCESSOR_ARCHITECTURE

    Write-Host "Registering with pike server..."
    try {
        $response = Invoke-WebRequest -Uri "{{BASE_URL}}/cb" -Method POST -Body "$hostName|exe|$arch" -UseBasicParsing
    } catch {
        $msg = "Server returned an error"
        if ($_.Exception.Response) {
            $code = [int]$_.Exception.Response.StatusCode
            $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
            $body = $reader.ReadToEnd()
            $reader.Close()
            $msg = "Server returned HTTP ${code}: $body"
        }
        Write-Error "ERROR: $msg"
        exit 1
    }
    $parts = $response.Content -split '\|'
    $fileName = $parts[0]
    $sha256 = $parts[1]

    $sensorPath = Join-Path $tmpDir $fileName

    Write-Host "Downloading sensor..."
    try {
        Invoke-WebRequest -Uri "{{BASE_URL}}/s/$fileName" -OutFile $sensorPath -UseBasicParsing
    } catch {
        $code = if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { "unknown" }
        Write-Error "ERROR: Failed to download sensor (HTTP $code)."
        exit 1
    }

    Write-Host "Verifying checksum..."
    $hash = (Get-FileHash -Path $sensorPath -Algorithm SHA256).Hash
    if ($hash -ne $sha256) {
        Write-Error "Checksum mismatch! Expected $sha256, got $hash"
        exit 1
    }

    Write-Host "Installing sensor..."
    Start-Process -FilePath $sensorPath -ArgumentList "/install /quiet /norestart CID={{CID}} {{CLOUD_PARAM}}" -Wait -NoNewWindow

    # Report success
    try { Invoke-WebRequest -Uri "{{BASE_URL}}/done" -Method POST -Body "$hostName|ok" -UseBasicParsing | Out-Null } catch {}

    Write-Host "CrowdStrike Falcon sensor installed successfully."
} finally {
    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
