// Validate semver format
export function parseSemver(version) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-([a-zA-Z]+)\.(\d+))?$/)
  if (!match) {
    throw new Error(`Invalid semver format: ${version}`)
  }

  return {
    major: parseInt(match[1], 10),
    minor: parseInt(match[2], 10),
    patch: parseInt(match[3], 10),
    prerelease: match[4] || null,
    prereleaseVersion: match[5] ? parseInt(match[5], 10) : null,
  }
}

// Format version object back to string
function formatVersion(ver) {
  let version = `${ver.major}.${ver.minor}.${ver.patch}`
  if (ver.prerelease) {
    version += `-${ver.prerelease}.${ver.prereleaseVersion}`
  }
  return version
}

// Calculate next version
export function bumpVersion(currentVersion, type, channel) {
  const ver = parseSemver(currentVersion)
  const isStable = channel === 'stable'

  // If bumping from prerelease to stable, just remove prerelease tag
  if (isStable && ver.prerelease) {
    ver.prerelease = null
    ver.prereleaseVersion = null
    return formatVersion(ver)
  }

  // If same channel as current prerelease, only increment prerelease version
  if (!isStable && ver.prerelease === channel) {
    ver.prereleaseVersion += 1
    return formatVersion(ver)
  }

  // Create initial prerelease without bumping patch
  if (!isStable && !ver.prerelease && type === 'patch') {
    ver.prerelease = channel
    ver.prereleaseVersion = 1
    return formatVersion(ver)
  }

  // Bump version numbers (new channel or stable release)
  if (type === 'major') {
    ver.major += 1
    ver.minor = 0
    ver.patch = 0
  } else if (type === 'minor') {
    ver.minor += 1
    ver.patch = 0
  } else if (type === 'patch') {
    ver.patch += 1
  } else {
    throw new Error(`Invalid bump type: ${type}. Must be patch, minor, or major.`)
  }

  // Handle prerelease channels
  if (!isStable) {
    ver.prerelease = channel
    ver.prereleaseVersion = 1
  } else {
    ver.prerelease = null
    ver.prereleaseVersion = null
  }

  return formatVersion(ver)
}
