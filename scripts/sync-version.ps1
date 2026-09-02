# Windows-friendly wrapper. Prefer: pnpm version:sync | version:check
param(
  [ValidateSet('sync', 'check', 'print')]
  [string]$Mode = 'sync'
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
node ./scripts/sync-version.mjs $Mode
