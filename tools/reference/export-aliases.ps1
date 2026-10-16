# Development-only metadata exporter. Does not enumerate managed providers.
$ErrorActionPreference = 'Stop'
function Convert-AliasValue($v) {
    if ($null -eq $v) { return $null }
    if ($v -is [System.Management.ManagementBaseObject]) {
        $o = [ordered]@{}
        # psbase is essential: MSFT_CliFormat has a WMI member named Properties.
        foreach ($p in $v.psbase.Properties) {
            if ($p.Name -eq 'Password' -and $null -ne $p.Value -and $p.Value -ne '') {
                throw 'Refusing to export an alias containing a password.'
            }
            $o[$p.Name] = Convert-AliasValue $p.Value
        }
        return [pscustomobject]$o
    }
    if ($v -is [System.Array]) {
        $a = @(foreach ($x in $v) { Convert-AliasValue $x })
        return ,$a
    }
    return $v
}
$data = @(foreach ($a in (Get-WmiObject -Namespace root\cli -Class MSFT_CliAlias | Sort-Object FriendlyName)) { Convert-AliasValue $a })
$json = ConvertTo-Json -InputObject $data -Depth 100 -Compress
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
