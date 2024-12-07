Write-Host "Installing GTK4 and dependencies for $env:VCPKG_TRIPLET..."
vcpkg install pkgconf:$env:VCPKG_TRIPLET
vcpkg install glib:$env:VCPKG_TRIPLET
vcpkg install gtk4:$env:VCPKG_TRIPLET
vcpkg install libadwaita:$env:VCPKG_TRIPLET
