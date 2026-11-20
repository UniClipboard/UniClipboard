param([Parameter(Mandatory=$true)][string]$ReceiptPath,[Parameter(Mandatory=$true)][string]$WindowTitle)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
$form=New-Object Windows.Forms.Form
$form.Text=$WindowTitle
$form.Width=520
$form.Height=180
$form.StartPosition='CenterScreen'
$label=New-Object Windows.Forms.Label
$label.Text='Synthetic task-owned shortcut target. No clipboard integration.'
$label.AutoSize=$true
$label.Left=16
$label.Top=20
$form.Controls.Add($label)
$form.Add_Shown({
    $process=[Diagnostics.Process]::GetCurrentProcess()
    @{pid=$process.Id;created_utc=$process.StartTime.ToUniversalTime().ToString('o');hwnd=$form.Handle.ToInt64();title=$WindowTitle}|ConvertTo-Json|Set-Content -Encoding UTF8 -LiteralPath $ReceiptPath
    [void]$form.Activate()
})
[void]$form.ShowDialog()
