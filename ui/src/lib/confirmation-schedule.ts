export function nextAutomaticConfirmationCheck(value: [] | [bigint]): bigint | undefined {
  return value[0]
}

export function hasScheduledConfirmation(values: ReadonlyArray<[] | [bigint]>): boolean {
  return values.some((value) => nextAutomaticConfirmationCheck(value) !== undefined)
}

export function shouldPollScheduledHistory(hasScheduledRecord: boolean, pageVisible: boolean): boolean {
  return hasScheduledRecord && pageVisible
}

export function automaticConfirmationCheckDate(nextCheckAtNs: bigint): Date {
  return new Date(Number(nextCheckAtNs / 1_000_000n))
}
