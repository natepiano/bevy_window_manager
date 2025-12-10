# Find all windows.ron files in AppData\Roaming
Get-ChildItem -Path "$env:APPDATA" -Recurse -Filter "windows.ron" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Found: $($_.FullName)"
    Write-Host "Contents:"
    Get-Content $_.FullName
    Write-Host ""
}
