// Recent delegations from the usage JSONL.
import type { UsageEvent } from '../lib/api'
import { Empty, Pill, Section, timeAgo } from './ui'

export function ActivityView({ usage }: { usage: UsageEvent[] }) {
  return (
    <Section
      title="Activity"
      subtitle="Recent delegations — which slot, which backend served it, latency, and failures."
    >
      {usage.length === 0 ? (
        <Empty icon="⋯">No delegations yet. Once an agent calls the delegate tool, calls appear here.</Empty>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>When</th>
              <th>Slot</th>
              <th>Backend</th>
              <th>Model</th>
              <th>Latency</th>
              <th>Result</th>
            </tr>
          </thead>
          <tbody>
            {usage.map((u, i) => (
              <tr key={i} className={u.success ? '' : 'row-err'}>
                <td title={u.ts}>{timeAgo(u.ts)}</td>
                <td className="mono">{u.slot}</td>
                <td className="mono dim">{u.profile_id ?? u.base_url}</td>
                <td className="mono">{u.model}</td>
                <td>{u.latency_ms} ms</td>
                <td>
                  {u.success ? (
                    <Pill tone="ok">ok</Pill>
                  ) : (
                    <Pill tone="err">{u.reason ? u.reason.slice(0, 60) : 'failed'}</Pill>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Section>
  )
}
