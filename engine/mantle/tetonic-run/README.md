# tetonic-run

Managed execution and the durable run/task/attempt supervisor.

`DurableRunSupervisor::new` is an execution-owner startup operation: it performs
recovery and may write recovered projections. Live inspection must use
`DurableRunReader`, which exposes only snapshots and journal replay, does not run
startup recovery, and has no command interface. The application remains responsible
for credential and information-context authorization around these reads.

The [local UI connection](../../../docs/implementation/contracts/local-ui-v1.md)
uses this reader while the existing managed supervisor continues to own execution.
