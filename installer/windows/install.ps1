# Installs Mammoth for the current user. No administrator rights are needed.
#
# The setup program runs this script with -Gui from the folder it unpacks to,
# beside mammoth.zip, and the script shows an installer window. Without -Gui
# it installs from the console, which also works from an unpacked release:
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Gui] [-InstallDir <folder>] [-NoPath] [-NoRegister] [-Quiet]
#
# -ScreenshotDir is for testing the window: it clicks through every page on
# its own and saves a picture of each page there.

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Mammoth'),
    [string]$Payload = '',
    [switch]$Gui,
    [switch]$NoPath,
    [switch]$NoRegister,
    [switch]$Quiet,
    [string]$ScreenshotDir = ''
)

$ErrorActionPreference = 'Stop'
$Marker = 'mammoth-install.txt'
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Mammoth'
if (-not $Payload) { $Payload = Join-Path $PSScriptRoot 'mammoth.zip' }

function Send-EnvironmentChange {
    # Tells Explorer and new terminals that PATH changed, without a sign-out.
    if (-not ('Mammoth.Environment' -as [type])) {
        Add-Type -Namespace Mammoth -Name Environment -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out System.UIntPtr lpdwResult);
'@
    }
    $result = [UIntPtr]::Zero
    [void][Mammoth.Environment]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
}

function Add-UserPath([string]$directory) {
    # Read PATH unexpanded and keep its registry type, so entries such as
    # %USERPROFILE%\bin still work afterwards.
    $key = Get-Item 'HKCU:\Environment'
    $current = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    $entries = @($current -split ';' | Where-Object { $_ -ne '' })
    if ($entries | Where-Object { $_.TrimEnd('\') -ieq $directory.TrimEnd('\') }) { return $false }
    $kind = if ($current -eq '') { 'ExpandString' } else { $key.GetValueKind('Path') }
    $updated = (@($entries) + $directory) -join ';'
    Set-ItemProperty -Path 'HKCU:\Environment' -Name 'Path' -Value $updated -Type $kind
    Send-EnvironmentChange
    return $true
}

# The version in the release zip's top folder, such as mammoth-0.1.0-windows-x64.
function Get-PackageVersion {
    if (Test-Path $Payload) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($Payload)
        try {
            foreach ($entry in $zip.Entries) {
                if ($entry.FullName -match '^mammoth-([^/\\]+)-windows-[^/\\]+[/\\]') { return $Matches[1] }
            }
        } finally { $zip.Dispose() }
    }
    # Run from an unpacked release folder.
    $compiler = Join-Path $PSScriptRoot 'bin\mammoth.exe'
    if (Test-Path $compiler) { return (& $compiler --version) -replace '^mammoth ', '' }
    return ''
}

# Why a folder cannot take the install, or '' when it can.
function Test-InstallDir([string]$directory) {
    if (-not $directory) { return 'Choose a folder to install into.' }
    try { $full = [IO.Path]::GetFullPath($directory) } catch { return 'That is not a valid folder name.' }
    if ((Test-Path -LiteralPath $full) -and -not (Test-Path -LiteralPath $full -PathType Container)) {
        return 'That is a file, not a folder.'
    }
    if (Test-Path -LiteralPath $full) {
        $items = @(Get-ChildItem -LiteralPath $full -Force)
        if ($items.Count -gt 0 -and -not (Test-Path (Join-Path $full $Marker))) {
            return 'That folder already has other files in it. Choose an empty folder or a new one.'
        }
    }
    return ''
}

# Installs into $directory, calling $report with a percentage and a status line.
# Returns the installed version. Throws when something fails, leaving any
# earlier install as it was unless the failure comes after the swap.
function Install-Mammoth([string]$directory, [bool]$addPath, [bool]$register, [scriptblock]$report) {
    $directory = [IO.Path]::GetFullPath($directory)
    $problem = Test-InstallDir $directory
    if ($problem) { throw $problem }

    # Unpack beside the target first, so a failure leaves any old install alone.
    $staging = "$directory.new"
    if (Test-Path $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    New-Item -ItemType Directory -Path $staging | Out-Null
    try {
        return Install-FromStaging $directory $staging $addPath $register $report
    } catch {
        if (Test-Path $staging) { Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue }
        throw
    }
}

function Install-FromStaging([string]$directory, [string]$staging, [bool]$addPath, [bool]$register, [scriptblock]$report) {
    $stagingRoot = [IO.Path]::GetFullPath($staging).TrimEnd('\') + '\'
    if (Test-Path $Payload) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($Payload)
        try {
            $entries = $zip.Entries
            $done = 0
            foreach ($entry in $entries) {
                $target = [IO.Path]::GetFullPath((Join-Path $staging $entry.FullName))
                if (-not $target.StartsWith($stagingRoot, [StringComparison]::OrdinalIgnoreCase)) {
                    throw "The package has a file outside its folder: $($entry.FullName)"
                }
                if ($entry.FullName.EndsWith('/') -or $entry.FullName.EndsWith('\')) {
                    New-Item -ItemType Directory -Force -Path $target | Out-Null
                } else {
                    New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
                    [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
                }
                $done++
                if ($done % 25 -eq 0 -or $done -eq $entries.Count) {
                    & $report ([int](5 + 85 * $done / $entries.Count)) "Copying files ($done of $($entries.Count))"
                }
            }
        } finally { $zip.Dispose() }
    } elseif (Test-Path (Join-Path $PSScriptRoot 'bin\mammoth.exe')) {
        # Run from an unpacked release folder.
        & $report 40 'Copying files'
        Get-ChildItem -LiteralPath $PSScriptRoot -Force | Copy-Item -Destination $staging -Recurse
    } else {
        throw 'Could not find mammoth.zip next to the installer.'
    }

    # A release zip holds one top folder; install its contents.
    $inner = @(Get-ChildItem -LiteralPath $staging -Directory)
    if ($inner.Count -eq 1 -and (Test-Path (Join-Path $inner[0].FullName 'bin\mammoth.exe'))) {
        $root = $inner[0].FullName
    } else {
        $root = $staging
    }
    $compiler = Join-Path $root 'bin\mammoth.exe'
    if (-not (Test-Path $compiler)) { throw 'The package is missing bin\mammoth.exe.' }
    & $report 92 'Checking the compiler starts'
    $version = (& $compiler --version) -replace '^mammoth ', ''
    if ($LASTEXITCODE -ne 0) { throw 'The packaged compiler did not start.' }

    & $report 95 'Finishing'
    if (Test-Path $directory) { Remove-Item -LiteralPath $directory -Recurse -Force }
    Move-Item -LiteralPath $root -Destination $directory
    if (Test-Path $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    Set-Content -LiteralPath (Join-Path $directory $Marker) -Encoding UTF8 -Value @(
        "Mammoth $version",
        "Installed $(Get-Date -Format s)",
        'This file marks the folder as a Mammoth install. uninstall.ps1 removes the folder.'
    )

    $bin = Join-Path $directory 'bin'
    if ($addPath) {
        if (Add-UserPath $bin) { & $report 97 "Added $bin to your PATH" }
    }
    if ($register) {
        $uninstall = Join-Path $directory 'uninstall.ps1'
        $size = [int]((Get-ChildItem -LiteralPath $directory -Recurse -File | Measure-Object Length -Sum).Sum / 1KB)
        New-Item -Path $UninstallKey -Force | Out-Null
        $values = @{
            DisplayName          = "Mammoth $version"
            DisplayVersion       = $version
            Publisher            = 'Mammoth'
            InstallLocation      = $directory
            DisplayIcon          = $compiler.Replace($root, $directory)
            UninstallString      = "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$uninstall`""
            QuietUninstallString = "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$uninstall`" -Quiet"
        }
        foreach ($name in $values.Keys) { Set-ItemProperty -Path $UninstallKey -Name $name -Value $values[$name] }
        Set-ItemProperty -Path $UninstallKey -Name NoModify -Value 1 -Type DWord
        Set-ItemProperty -Path $UninstallKey -Name NoRepair -Value 1 -Type DWord
        Set-ItemProperty -Path $UninstallKey -Name EstimatedSize -Value $size -Type DWord
    }
    & $report 100 'Done'
    return $version
}

function Invoke-Console {
    function Finish([int]$code, [string]$message) {
        if ($message) { Write-Host $message }
        if (-not $Quiet) { Read-Host 'Press Enter to close' | Out-Null }
        exit $code
    }
    try {
        $full = [IO.Path]::GetFullPath($InstallDir)
        Write-Host "Installing Mammoth to $full"
        $version = Install-Mammoth $full (-not $NoPath) (-not $NoRegister) {
            param($percent, $text)
            if ($text -like 'Added *') { Write-Host $text }
        }
        $bin = Join-Path $full 'bin'
        $hint = if ($NoPath) { "& `"$bin\mammoth.exe`"" } else { 'mammoth' }
        Finish 0 @"

Mammoth $version is installed. Java is included, so nothing else is needed.
Open a new terminal and try:

    $hint --version
    $hint "$full\examples\greet.mam"

Remove it from Settings > Apps > Installed apps, or run uninstall.ps1 in $full.
"@
    } catch {
        Finish 1 "Installing Mammoth failed: $($_.Exception.Message)"
    }
}

function Invoke-Gui {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    if (-not ('Mammoth.Dpi' -as [type])) {
        Add-Type -Namespace Mammoth -Name Dpi -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
    }
    [void][Mammoth.Dpi]::SetProcessDPIAware()
    [Windows.Forms.Application]::EnableVisualStyles()

    $version = Get-PackageVersion
    $title = if ($version) { "Mammoth $version Setup" } else { 'Mammoth Setup' }
    $font = New-Object Drawing.Font('Segoe UI', 9)
    $state = @{ Installed = ''; Error = '' }

    $form = New-Object Windows.Forms.Form
    $form.Text = $title
    $form.Font = $font
    # Sizes below are for 96 DPI; the form scales them to the screen.
    $form.AutoScaleDimensions = New-Object Drawing.SizeF(96, 96)
    $form.AutoScaleMode = 'Dpi'
    $form.ClientSize = New-Object Drawing.Size(560, 380)
    $form.FormBorderStyle = 'FixedDialog'
    $form.MaximizeBox = $false
    $form.StartPosition = 'CenterScreen'
    try { $form.Icon = [Drawing.Icon]::ExtractAssociatedIcon((Join-Path $env:SystemRoot 'System32\cmd.exe')) } catch {}

    # The white band at the top, as Windows installers have.
    $header = New-Object Windows.Forms.Panel
    $header.Dock = 'Top'; $header.Height = 72; $header.BackColor = [Drawing.Color]::White
    $heading = New-Object Windows.Forms.Label
    $heading.Font = New-Object Drawing.Font('Segoe UI Semibold', 13)
    $heading.Location = New-Object Drawing.Point(20, 12); $heading.AutoSize = $true
    $subheading = New-Object Windows.Forms.Label
    $subheading.Location = New-Object Drawing.Point(22, 42); $subheading.Size = New-Object Drawing.Size(520, 20)
    $subheading.AutoEllipsis = $true
    $subheading.ForeColor = [Drawing.Color]::FromArgb(90, 90, 90)
    $header.Controls.AddRange(@($heading, $subheading))
    $rule = New-Object Windows.Forms.Label
    $rule.Dock = 'Top'; $rule.Height = 1; $rule.BackColor = [Drawing.Color]::FromArgb(220, 220, 220)

    $footer = New-Object Windows.Forms.Panel
    $footer.Dock = 'Bottom'; $footer.Height = 52
    $primary = New-Object Windows.Forms.Button
    $primary.Size = New-Object Drawing.Size(96, 28); $primary.Location = New-Object Drawing.Point(344, 12)
    $secondary = New-Object Windows.Forms.Button
    $secondary.Size = New-Object Drawing.Size(96, 28); $secondary.Location = New-Object Drawing.Point(448, 12)
    $footer.Controls.AddRange(@($primary, $secondary))
    $form.AcceptButton = $primary

    $body = New-Object Windows.Forms.Panel
    $body.Dock = 'Fill'; $body.Padding = New-Object Windows.Forms.Padding(22, 16, 22, 0)

    # Welcome page.
    $welcome = New-Object Windows.Forms.Panel
    $welcome.Dock = 'Fill'
    $intro = New-Object Windows.Forms.Label
    $intro.Location = New-Object Drawing.Point(0, 0); $intro.Size = New-Object Drawing.Size(516, 54)
    $intro.Text = "This installs the Mammoth compiler for your user account. Java is included, so you don't need to install it separately, and you don't need administrator rights."
    $folderLabel = New-Object Windows.Forms.Label
    $folderLabel.Text = 'Install folder:'; $folderLabel.AutoSize = $true
    $folderLabel.Location = New-Object Drawing.Point(0, 64)
    $folder = New-Object Windows.Forms.TextBox
    $folder.Location = New-Object Drawing.Point(0, 86); $folder.Size = New-Object Drawing.Size(410, 24)
    $folder.Text = [IO.Path]::GetFullPath($InstallDir)
    $browse = New-Object Windows.Forms.Button
    $browse.Text = 'Browse...'; $browse.Location = New-Object Drawing.Point(420, 84); $browse.Size = New-Object Drawing.Size(96, 28)
    $folderNote = New-Object Windows.Forms.Label
    $folderNote.Location = New-Object Drawing.Point(0, 116); $folderNote.Size = New-Object Drawing.Size(516, 20)
    $folderNote.ForeColor = [Drawing.Color]::FromArgb(90, 90, 90)
    $pathBox = New-Object Windows.Forms.CheckBox
    $pathBox.Text = 'Add Mammoth to PATH, so the mammoth command works in any new terminal'
    $pathBox.Location = New-Object Drawing.Point(0, 146); $pathBox.Size = New-Object Drawing.Size(516, 24)
    $pathBox.Checked = -not $NoPath
    $license = New-Object Windows.Forms.LinkLabel
    $license.Text = 'Mammoth is licensed under Apache-2.0 WITH LLVM-exception. View the license'
    $license.LinkArea = New-Object Windows.Forms.LinkArea($license.Text.IndexOf('View'), 'View the license'.Length)
    $license.Location = New-Object Drawing.Point(0, 186); $license.Size = New-Object Drawing.Size(516, 20)
    $welcome.Controls.AddRange(@($intro, $folderLabel, $folder, $browse, $folderNote, $pathBox, $license))

    # Progress page.
    $progressPage = New-Object Windows.Forms.Panel
    $progressPage.Dock = 'Fill'; $progressPage.Visible = $false
    $status = New-Object Windows.Forms.Label
    $status.Location = New-Object Drawing.Point(0, 10); $status.Size = New-Object Drawing.Size(516, 20)
    $bar = New-Object Windows.Forms.ProgressBar
    $bar.Location = New-Object Drawing.Point(0, 38); $bar.Size = New-Object Drawing.Size(516, 22)
    $bar.Minimum = 0; $bar.Maximum = 100
    $progressPage.Controls.AddRange(@($status, $bar))

    # Finish page, also used to report a failure.
    $finishPage = New-Object Windows.Forms.Panel
    $finishPage.Dock = 'Fill'; $finishPage.Visible = $false
    $result = New-Object Windows.Forms.Label
    $result.Location = New-Object Drawing.Point(0, 0); $result.Size = New-Object Drawing.Size(516, 120)
    $commands = New-Object Windows.Forms.TextBox
    $commands.Location = New-Object Drawing.Point(0, 124); $commands.Size = New-Object Drawing.Size(516, 48)
    $commands.Multiline = $true; $commands.ReadOnly = $true; $commands.WordWrap = $false
    $commands.Font = New-Object Drawing.Font('Consolas', 10)
    $examples = New-Object Windows.Forms.LinkLabel
    $examples.Text = 'Open the examples folder'
    $examples.Location = New-Object Drawing.Point(0, 186); $examples.AutoSize = $true
    $finishPage.Controls.AddRange(@($result, $commands, $examples))

    $body.Controls.AddRange(@($welcome, $progressPage, $finishPage))
    $form.Controls.AddRange(@($body, $footer, $rule, $header))

    function Update-FolderNote {
        $problem = Test-InstallDir $folder.Text
        if ($problem) {
            $folderNote.Text = $problem; $folderNote.ForeColor = [Drawing.Color]::Firebrick
            $primary.Enabled = $false; $primary.Text = 'Install'
        } else {
            $existing = Test-Path (Join-Path $folder.Text $Marker)
            $folderNote.Text = if ($existing) { 'Mammoth is already installed here. Installing replaces it.' } else { '' }
            $folderNote.ForeColor = [Drawing.Color]::FromArgb(90, 90, 90)
            $primary.Enabled = $true
            $primary.Text = if ($existing) { 'Reinstall' } else { 'Install' }
        }
    }

    function Show-Page([string]$name) {
        $welcome.Visible = $name -eq 'welcome'
        $progressPage.Visible = $name -eq 'progress'
        $finishPage.Visible = $name -eq 'finish'
        switch ($name) {
            'welcome' {
                $heading.Text = 'Install Mammoth'
                $subheading.Text = if ($version) { "Version $version, an experimental preview" } else { '' }
                $secondary.Text = 'Cancel'; $secondary.Enabled = $true
                Update-FolderNote
            }
            'progress' {
                $heading.Text = 'Installing'
                $subheading.Text = 'This takes a few seconds.'
                $primary.Enabled = $false; $secondary.Enabled = $false
            }
            'finish' {
                $secondary.Enabled = $true
                if ($state.Error) {
                    $heading.Text = 'Mammoth was not installed'
                    $subheading.Text = 'The reason is below. Choose Back to try again.'
                    $result.Text = "Installing failed:`r`n`r`n$($state.Error)"
                    $commands.Visible = $false; $examples.Visible = $false
                    $primary.Text = 'Back'; $primary.Enabled = $true; $secondary.Text = 'Close'
                } else {
                    $heading.Text = 'Mammoth is installed'
                    $subheading.Text = "Version $($state.Installed) is in $($state.Directory)"
                    $pathText = if ($state.AddedPath) { 'Open a new terminal and run:' } else { 'Mammoth was not added to PATH, so run it by its full path:' }
                    $result.Text = "You can start writing Mammoth programs now. $pathText"
                    $result.Height = 40
                    $commands.Location = New-Object Drawing.Point(0, 44)
                    $mammoth = if ($state.AddedPath) { 'mammoth' } else { "& `"$($state.Directory)\bin\mammoth.exe`"" }
                    $commands.Text = "$mammoth --version`r`ncd `"$($state.Directory)\examples`"`r`n$mammoth greet.mam"
                    # Room for three lines, plus a scroll bar when a path is long.
                    $lines = $commands.Font.Height * 3 + 8
                    $long = ($commands.Text -split "`r`n" | Measure-Object Length -Maximum).Maximum -gt 64
                    $commands.ScrollBars = if ($long) { 'Horizontal' } else { 'None' }
                    $commands.Height = $lines + $(if ($long) { [Windows.Forms.SystemInformation]::HorizontalScrollBarHeight } else { 0 })
                    $examples.Location = New-Object Drawing.Point(0, ($commands.Bottom + 12))
                    $commands.Visible = $true; $examples.Visible = $true
                    $primary.Visible = $false; $secondary.Text = 'Finish'
                    $form.AcceptButton = $secondary
                }
            }
        }
    }

    function Save-Page([string]$name) {
        if (-not $ScreenshotDir) { return }
        New-Item -ItemType Directory -Force -Path $ScreenshotDir | Out-Null
        $form.Refresh()
        $image = New-Object Drawing.Bitmap($form.Width, $form.Height)
        $form.DrawToBitmap($image, (New-Object Drawing.Rectangle(0, 0, $form.Width, $form.Height)))
        $image.Save((Join-Path $ScreenshotDir "$name.png"), [Drawing.Imaging.ImageFormat]::Png)
        $image.Dispose()
    }

    function Start-Install {
        $problem = Test-InstallDir $folder.Text
        if ($problem) { Update-FolderNote; return }
        $state.Directory = [IO.Path]::GetFullPath($folder.Text)
        $state.AddedPath = $pathBox.Checked
        $state.Error = ''
        Show-Page 'progress'
        $saved = @{ Middle = $false }
        $report = {
            param($percent, $text)
            $bar.Value = [Math]::Min(100, [Math]::Max(0, $percent))
            $status.Text = $text
            if ($percent -ge 50 -and -not $saved.Middle) { $saved.Middle = $true; Save-Page '2-progress' }
            [Windows.Forms.Application]::DoEvents()
        }
        try {
            $state.Installed = Install-Mammoth $state.Directory $pathBox.Checked (-not $NoRegister) $report
        } catch {
            $state.Error = $_.Exception.Message
        }
        Show-Page 'finish'
    }

    $browse.Add_Click({
        $dialog = New-Object Windows.Forms.FolderBrowserDialog
        $dialog.Description = 'Choose where to install Mammoth. A new folder named Mammoth inside it works well.'
        $dialog.ShowNewFolderButton = $true
        if (Test-Path $folder.Text) { $dialog.SelectedPath = $folder.Text }
        if ($dialog.ShowDialog($form) -eq 'OK') { $folder.Text = $dialog.SelectedPath }
    })
    $folder.Add_TextChanged({ Update-FolderNote })
    $license.Add_LinkClicked({
        $text = ''
        $local = Join-Path $PSScriptRoot 'LICENSE'
        if (Test-Path $local) { $text = Get-Content -Raw $local }
        $view = New-Object Windows.Forms.Form
        $view.Text = 'Mammoth license'; $view.Font = $font; $view.StartPosition = 'CenterParent'
        $view.ClientSize = New-Object Drawing.Size(640, 480)
        $box = New-Object Windows.Forms.TextBox
        $box.Multiline = $true; $box.ReadOnly = $true; $box.ScrollBars = 'Vertical'; $box.Dock = 'Fill'
        $box.Font = New-Object Drawing.Font('Consolas', 9)
        $box.Text = if ($text) { $text -replace "(?<!`r)`n", "`r`n" } else { 'The license is in the LICENSE file of the installed folder.' }
        $view.Controls.Add($box)
        [void]$view.ShowDialog($form)
    })
    $examples.Add_LinkClicked({ Start-Process explorer.exe (Join-Path $state.Directory 'examples') })
    $primary.Add_Click({ if ($state.Error) { Show-Page 'welcome' } else { Start-Install } })
    $secondary.Add_Click({ $form.Close() })

    if ($ScreenshotDir) {
        # Testing: click through the pages and save each one.
        $state.Timer = New-Object Windows.Forms.Timer
        $state.Timer.Interval = 400
        $state.Step = 0
        $state.Timer.Add_Tick({
            try {
                $state.Step++
                switch ($state.Step) {
                    1 { Save-Page '1-welcome' }
                    2 { $state.Timer.Stop(); Start-Install; $state.Timer.Start() }
                    3 { Save-Page '3-finish' }
                    default { $state.Timer.Stop(); $form.Close() }
                }
            } catch {
                $state.Timer.Stop()
                Add-Content -LiteralPath (Join-Path $ScreenshotDir 'errors.txt') -Value "$($_.Exception.Message) $($_.InvocationInfo.PositionMessage)"
                $form.Close()
            }
        })
        $form.Add_Shown({ $state.Timer.Start() })
    }

    Show-Page 'welcome'
    [void]$form.ShowDialog()
    if ($state.Error) { exit 1 }
    if (-not $state.Installed) { exit 2 }
    exit 0
}

if ($Gui -or $ScreenshotDir) { Invoke-Gui } else { Invoke-Console }
