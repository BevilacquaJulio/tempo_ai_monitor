export const isUsageFilePath = (path: string) => /^[a-z]:[/\\].+\.json$/i.test(path.trim())
