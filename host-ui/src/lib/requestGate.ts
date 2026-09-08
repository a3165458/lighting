export class RequestGate {
  private revision = 0
  private activeMutations = 0

  beginRefresh(): number | null {
    if (this.activeMutations > 0) return null
    this.revision += 1
    return this.revision
  }

  beginMutation(): number {
    this.activeMutations += 1
    this.revision += 1
    return this.revision
  }

  finishMutation(): void {
    this.activeMutations = Math.max(0, this.activeMutations - 1)
  }

  isCurrent(revision: number): boolean {
    return revision === this.revision
  }
}
