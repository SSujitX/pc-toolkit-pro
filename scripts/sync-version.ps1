# Windows-friendly wrapper. Prefer: pnpm version:sync | pnpm version:check
# Bump: .\scripts\sync-version.ps1 bump patch
param(
  [ValidateSet('sync', 'check', 'print', 'bump')]
  [string]$Mode = 'sync',
  [Parameter(Position = 1)]
  [string]$Bump = ''
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
if ($Mode -eq 'bump') {
  if (-not $Bump) { throw 'bump requires current|patch|minor|major' }
  node ./scripts/sync-version.mjs bump $Bump
} else {
  node ./scripts/sync-version.mjs $Mode
}
