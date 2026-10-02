# Synthetic IPC peer. Input is data, never PowerShell source.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PipePeer {
    [DllImport("kernel32.dll")]
    public static extern IntPtr GetStdHandle(int id);
}
'@
$handle = [Microsoft.Win32.SafeHandles.SafePipeHandle]::new([PipePeer]::GetStdHandle(-10), $false)
$pipe = [System.IO.Pipes.NamedPipeServerStream]::new([System.IO.Pipes.PipeDirection]::InOut, $true, $true, $handle)
$utf8 = [System.Text.UTF8Encoding]::new($false)
$reader = [System.IO.StreamReader]::new($pipe, $utf8, $false, 4096, $true)
$writer = [System.IO.StreamWriter]::new($pipe, $utf8, 4096, $true)
$writer.AutoFlush = $true
$writer.WriteLine($reader.ReadLine())
$reader.Dispose()
$writer.Dispose()
$pipe.Dispose()
