Start-Process powershell -Verb runAs -ArgumentList "-Command `"Set-Location -Path '$(Get-Location)'; Copy-Item '.\target\release\mpvrun.exe' 'C:\Program Files\mpv\mpvrun.exe' -Force`""
