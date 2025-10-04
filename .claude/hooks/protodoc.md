# Complete Acronym Protocol Reference

## Eliminating Ambiguity in AI & Agent Communication

---

## Original AI Communication Framework

### Core Operations

- **VET** = "Validate input, Error handling, Type checking"
- **SHIP** = "Security review, Health checks, Integration tests, Performance benchmarks"
- **TRACE** = "Timestamp, Request ID, Actor, Context, Event"
- **SCAN** = "Structure, Commands, Artifacts, Namespaces"
- **BUILD** = "Baseline, Unite, Inherit, Localize, Deliver"
- **VALID** = "Verify syntax, Assert executability, Link references, Identify conflicts, Document decisions"
- **CORE** = "Commands, Organization, Runtime, Environment"
- **TEST** = "Tools, Execution, Syntax, Targets"
- **DEPS** = "Direct, External, Peer, System"

---

## Planning Methodologies

### Military/Tactical

- **PACE** = Primary, Alternate, Contingency, Emergency
- **CARVER** = Criticality, Accessibility, Recuperability, Vulnerability, Effect, Recognizability
- **OODA** = Observe, Orient, Decide, Act

### Emergency Response

- **SALT** = Sort, Assess, Lifesaving interventions, Treatment/Transport
- **LAST** = Locate, Access, Stabilize, Transport

### Project Management

- **MoSCoW** = Must have, Should have, Could have, Won't have

---

## Technical Standards

### Software Engineering

- **CRUD** = Create, Read, Update, Delete
- **ACID** = Atomicity, Consistency, Isolation, Durability
- **DRY** = Don't Repeat Yourself
- **RAID** = Redundant Array of Independent Disks (levels 0, 1, 5, 6, 10)
- **RPO/RTO** = Recovery Point Objective / Recovery Time Objective
- **RFC 2119** = MUST, MUST NOT, SHOULD, SHOULD NOT, MAY (requirements language)

---

## Code Quality & Architecture

- **CLEAN** = "Comments on complex logic, Logging at decision points, Error handling explicit, Assertions for invariants, Names self-documenting"
- **SOLID-CHECK** = "Single responsibility per class, Open for extension only, Liskov substitution valid, Interfaces segregated, Dependencies inverted"
- **ARCH** = "Abstractions defined, Resources managed, Coupling < 3, Hierarchy depth < 5"

---

## Performance & Optimization

- **SPEED** = "Sub-second response, Pagination at 100 items, Eager load relationships, Expire cache in 5 minutes, Database queries < 10"
- **SCALE** = "Supports 10x load, Caches 80% requests, Async all I/O, Limits rate to 100/minute, Elastic resource allocation"
- **METRIC** = "Memory < 512MB, Execution < 2 seconds, Throughput > 1000/sec, Response p99 < 500ms, Idle < 10% CPU"

---

## Data & State Management

- **STORE** = "Sanitize all inputs, Transact atomically, Optimize indexes, Retain 30 days, Encrypt at rest"
- **SYNC** = "Source of truth defined, Yield on conflicts, No partial writes, Checksum validation"
- **PURGE** = "PII after 90 days, Unused after 180 days, Revisions keep 10, Garbage collect weekly, Exports delete after 7 days"
- **STATE** = "Snapshot on change, Timestamp UTC, Ancestry tracked, Type enforced, Expiry defined"
- **PERSIST** = "Primary location, Encryption required, Replicas minimum 2, Schema versioned, Index defined, Size limit 10MB, TTL specified"
- **RESTORE** = "Reference point ID, Endpoint specified, State validated, Time limit 60s, Order preserved, Relationships intact, Event fired"

---

## Error & Exception Handling

- **CATCH** = "Categorize by type, Alert if critical, Try recovery once, Context in logs, Handle at boundary"
- **FAIL** = "Fast within 5 seconds, Atomic (no partial state), Informative error codes, Logged with stack trace"
- **RETRY** = "Repeat 3 times, Exponential backoff (1, 2, 4 seconds), Track failure count, Return error after max, Yield different error if pattern"

---

## Documentation & Communication

- **DOCS** = "Description of purpose, Options listed with types, Code examples provided, Scenarios covered"
- **COMMENT** = "Complex logic explained, Ownership noted, Magic numbers defined, Metadata included, Edge cases documented, Non-obvious decisions justified, Trade-offs stated"
- **REPORT** = "Result stated first, Evidence provided, Patterns identified, Options compared, Recommendation explicit, Timeline included"

---

## Security & Authorization

- **GUARD** = "Gate at entry, Unique session tokens, Authenticate all requests, Rate limit by IP, Deny by default"
- **AUDIT** = "Actor recorded, UUID for request, Date timestamp UTC, IP address logged, Target resource identified"
- **ENCRYPT** = "Everything in transit, Names of PII fields, Credentials immediately, Random IV each time, Year-versioned algorithm, Password with bcrypt-12, Token with 256 bits"

---

## Testing & Validation

- **PROVE** = "Positive cases pass, Range boundaries tested, Other systems mocked, Validation errors caught, Edge cases covered"
- **CHECK** = "Contract validated, Happy path tested, Error paths tested, Cleanup confirmed, Kill switches work"
- **ASSERT** = "Assumptions listed, Schemas validated, Size boundaries checked, Error paths tested, Results deterministic, Time bounded"
- **VERIFY** = "Validate output schema, Error rate < 5%, Results non-empty, Interface contract met, Format compliant, Yield metrics"
- **GRADE** = "Goal achievement scored, Results quality measured, Accuracy checked, Duration recorded, Efficiency calculated"

---

## Deployment & Release

- **DEPLOY** = "Dependencies locked, Environment variables set, Permissions verified, Logs configured, Old versions archived, Yamls validated"
- **ROLLBACK** = "Revert < 5 minutes, Old version cached, Label previous stable, Load state preserved, Block new deploys, Announce in channel, Conduct post-mortem, Keep artifacts 30 days"

---

## User Interface & Experience

- **RESPONSIVE** = "Render < 100ms, Escape all user input, Show loading after 200ms, Paginate at 50 items, Optimize images < 100KB, Navigate without refresh, Support offline mode, Include keyboard shortcuts, Validate before submit, Error messages actionable"

---

## Agent Communication Standards

### Messaging Protocols

- **SPEAK** = "Schema defined, Payload validated, Error codes standardized, Acknowledgment required, Keep-alive every 30s"
- **LISTEN** = "Lock on single channel, Identify message source, Sequence number tracked, Timeout after 10s, Echo receipt, Notify on failure"
- **BRIDGE** = "Bidirectional channel, Rate limit 100/min, Identity verified, Dead letter queue, Graceful degradation, Exponential backoff"
- **MESH** = "Message routing table, Election for coordinator, Service discovery, Health check every 5s"
- **PULSE** = "Progress reported, Update interval 30s, Lock heartbeat, Status enumerated, Error broadcast"

### Input/Output Specifications

- **INTAKE** = "Identify format, Normalize encoding, Type check fields, Acknowledge receipt, Keep original, Emit parsed"
- **OUTPUT** = "Order guaranteed, UTF-8 encoded, Timestamp included, Paginated at 1000, Unique ID per item, Total count header"
- **STREAM** = "Start token sent, Timeout 30 seconds, Retry on disconnect, End token required, Acknowledge chunks, Metrics logged"
- **BATCH** = "Buffer 100 items, Atomic write, Transaction ID, Checksum included, Header with count"

---

## Development Cycle Protocols

- **EVOLVE** = "Extract current state, Version increment, Optimize identified bottlenecks, Lock during migration, Validate post-change, Emit change event"
- **BRANCH** = "Baseline locked, Resource isolated, Artifacts separate, Name follows pattern, Changelog required, History preserved"
- **MERGE** = "Migrations ordered, Environment tested, Rollback prepared, Gate conditions met, Event broadcast"
- **CYCLE** = "Context preserved, Yield after timeout, Cache intermediate, Loop detection, Exit conditions defined"

---

## Agent Capability Protocols

- **INVOKE** = "Intent declared, Name exact match, Version specified, Output schema defined, Kill switch enabled, Error contract explicit"
- **DELEGATE** = "Dependency declared, Endpoint verified, Load balanced, Error bubbled, Graceful fallback, Audit logged, Timeout enforced, Event published"
- **COMPOSE** = "Components listed, Order specified, Memory limit 512MB, Pipeline validated, Output chained, State preserved, Error halts"
- **PROBE** = "Performance measured, Resource monitored, Output validated, Behavior logged, Errors categorized"

---

## Orchestration Standards

- **CONDUCT** = "Coordinator elected, Order defined, Next agent specified, Data transformed, Unique ID tracked, Context passed, Timeout cascaded"
- **RELAY** = "Receive complete, Enrich if needed, Log transition, Acknowledge forward, Yield on backpressure"
- **WEAVE** = "Workflows defined, Events subscribed, Actions atomic, Version locked, Error compensated"

---

## Task Decomposition & Delegation

### Task Analysis & Breakdown

- **SPLIT** = "Scope under 4 hours, Parallel when possible, Language domain-specific, Interface defined, Type inputs/outputs"
- **ATOM** = "Actionable in one step, Testable independently, Output type defined, Measurable success criteria"
- **CHUNK** = "Context self-contained, Hours maximum 8, Understood without parent, Named descriptively, Knowledge requirements listed"
- **SLICE** = "Sequential dependencies mapped, Layer by abstraction, Input requirements complete, Checkpoint defined, Exit criteria explicit"

### Dependency & Sequencing

- **CHAIN** = "Child tasks identified, Handoff points defined, Ancestry tracked, Input from previous, Next task specified"
- **GRAPH** = "Gates defined, Resources mapped, Ancestors listed, Paths parallelized, Halt conditions specified"
- **ORDER** = "Orchestrate by priority, Resource prerequisites, Dependencies resolved, Execution sequence, Results aggregation point"
- **BLOCK** = "Boundary defined, Lock requirements, Output contract, Critical path marked, Kill cascade specified"

### Team Assignment & Routing

- **ROUTE** = "Requirements matched, Ownership assigned, Urgency scored 1-5, Team capacity checked, Escalation path defined"
- **ASSIGN** = "Agent capabilities verified, Skills matrix matched, Schedule confirmed, Identity recorded, Grant permissions, Notify recipient"
- **SQUAD** = "Specialization identified, Quota per agent, Unified interface, Arbitrator designated, Distribution algorithm"
- **MATCH** = "Model capabilities required, Availability confirmed, Task type aligned, Complexity scored, History considered"

### Work Package Definition

- **BRIEF** = "Background provided, Requirements listed, Input examples given, Expected output shown, Failure cases defined"
- **PACKET** = "Purpose stated, Assets included, Context preserved, Knowledge requirements, Execution timeout, Test cases provided"
- **BUNDLE** = "Batch related tasks, Units maximum 10, Named collection, Dependencies internal, Leader task identified, Events coordinated"
- **TICKET** = "Task ID unique, Instructions complete, Context included, Knowledge base linked, Escalation threshold, Timeout specified"

### Delegation Control

- **HANDOFF** = "Hash state computed, Acknowledge transfer, Notify next agent, Documentation included, Ownership transferred, Forward context, Flag dependencies"
- **DISPATCH** = "Destination verified, Instructions packaged, Schedule confirmed, Priority assigned, Acknowledgment required, Timeout set, Channel specified, History logged"
- **RELEASE** = "Resources allocated, Execution authorized, Lock obtained, Environment prepared, Agent notified, Start time logged, End time estimated"
- **DEFER** = "Delay reason coded, Execute after timestamp, Forward to queue, Error if expired, Retry count limited"

### Coordination & Synchronization

- **RALLY** = "Rendezvous point set, Agents confirmed, Lock step execution, Leader elected, Yield on timeout"
- **SWARM** = "Scatter tasks equally, Workers scale 1-100, Aggregate results, Resource pool shared, Master coordinates"

### Result Assembly

- **GATHER** = "Gate on completion, Aggregate by type, Timeout 5 minutes, Handle partial results, Error tolerance 10%, Results ordered"
- **REDUCE** = "Receive all inputs, Eliminate duplicates, Deduplicate by ID, Unify format, Compress output, Error on mismatch"
- **STITCH** = "Sequential assembly, Thread ID tracked, Input validated, Transform applied, Check continuity, Hash final state"

### Escalation & Recovery

- **ELEVATE** = "Error threshold exceeded, Leader notified, Evidence collected, Version preserved, Alternatives attempted, Team lead engaged, Event logged"
- **RESCUE** = "Recovery attempted, Error isolated, State preserved, Context maintained, Unit retried, Event broadcast"

---

## Implementation Example

```python
# Example of using multiple protocols together
class TaskExecutor:
    def process_complex_task(self, task):
        # SPLIT the task into manageable chunks
        chunks = SPLITProtocol.decompose(task)  # <4 hour units

        # ROUTE each chunk to appropriate teams
        assignments = ROUTEProtocol.route(chunks)  # Match capabilities

        # DISPATCH with clear specifications
        for assignment in assignments:
            DISPATCHProtocol.send(
                destination=assignment.team,
                packet=PACKETProtocol.create(assignment),
                timeout=3600,
                retry=RETRYProtocol.config
            )

        # GATHER results with validation
        results = GATHERProtocol.collect(
            assignments,
            verify=VERIFYProtocol.validate
        )

        return results
```

---

## Usage Guidelines

1. **Composition**: Acronyms can be combined (e.g., `VET-CRITICAL` = VET + additional critical path validations)
2. **Versioning**: Each protocol can be versioned (e.g., `SPEAK/1.0`, `SPEAK/2.0`)
3. **Context**: Apply acronyms based on domain context (development, operations, communication)
4. **Measurement**: Each acronym contains measurable criteria, not subjective assessments
5. **Evolution**: New acronyms should follow the pattern of eliminating interpretation at execution time

---

## Claude Code Hook Protocols

### Hook Lifecycle Management

- **HOOK** = "Handler registered, Output validated, Operation atomic, Kill switch enabled"
- **INIT** = "Input parsed, Name verified, Interface loaded, Timeout configured"
- **EXEC** = "Environment validated, eXecute main logic, Exit code defined, Context preserved"
- **EMIT** = "Exit code returned, Message to stderr/stdout, Interface contract met, Timing logged"

### Hook Event Processing

- **PRETOOL** = "Permission check, Resource validation, Environment ready, Tool allowed, Output decision, Outcome logged"
- **POSTTOOL** = "Process result, Output feedback, Status check, Transform if needed, Orchestrate next, Output metrics, Log completion"
- **SESSION** = "State initialized, Environment loaded, Settings applied, Subagents ready, Input validated, Output context, Next step defined"

### Hook Validation Protocols

- **VALHOOK** = "Validate JSON input, Assert required fields, Link to configuration, Handle parse errors, Output structured, Orchestration ready, Kill on critical"
- **PATHVAL** = "Path traversal blocked, Absolute paths resolved, Target in scope, Hidden files checked, Validate permissions, Assert safety, Log violations"
- **SECVAL** = "Secrets pattern scan, Environment variables checked, Credentials masked, Validation strict, API keys detected, Log findings"

### Hook Configuration Standards

- **CONFIG** = "Configuration loaded, Options validated, Namespaces isolated, Format JSON, Input schema strict, Graceful defaults"
- **HOOKENV** = "Hook directory set, Output streams defined, Options from config, Kill switch ready, Environment variables, Namespace isolated, Version tracked"

### Hook Error Handling

- **HOOKERR** = "Handle gracefully, Output to stderr, Operation non-blocking, Kill if critical, Error code standard, Report context, Recovery attempted"
- **EXITCODE** = "Exit 0 for success, eXit 2 for blocking, Interface standard, Timeout respected, Context preserved, Output appropriate, Document reason, Emit to correct stream"

### Hook Communication

- **HOOKPIPE** = "Handle stdin JSON, Output structured data, Operation atomic, Kill on timeout, Parse errors handled, Interface versioned, Protocol documented, Emit metrics"
- **FEEDBACK** = "Format messages clearly, Error context included, Exit appropriately, Decision documented, Blocking justified, Alternatives suggested, Context preserved, Knowledge transferred"

### Hook Composition

- **COMPOSE** = "Chain validators, Order sequential, Memory shared, Pipeline validated, Output combined, State preserved, Errors propagated"
- **ORCHESTRA** = "Order hooks by priority, Register in settings, Coordinate execution, Handle dependencies, Execute in parallel, Synchronize results, Track metrics, Report completion"

### Hook Testing Protocols

- **HOOKTEST** = "Handler mocked, Output verified, Operation isolated, Kill tested, Timeout validated, Error cases covered, State cleaned, Types checked"
- **SIMULATE** = "Stdin mocked, Input variations tested, Mock tool responses, Understand edge cases, Load test performed, Assert outputs, Timeout tested, Error recovery verified"

### Hook Security Protocols

- **HOOKSEC** = "Handler sandboxed, Output sanitized, Operations restricted, Kill on violation, Secrets never logged, Environment isolated, Context limited"
- **SANDBOX** = "System calls limited, Access restricted, Network blocked, Directory confined, Binary execution denied, Output filtered, eXceptions caught"

### Hook Performance Metrics

- **HOOKPERF** = "Handler < 5 seconds, Output buffered, Optimization applied, Kill after timeout, Parse time < 100ms, Execution logged, Resource monitored, Feedback immediate"
- **METRICS** = "Memory < 50MB, Execution < 5s, Throughput measured, Response immediate, Input cached, CPU < 10%, Storage minimal"

---

## Hook Implementation Examples

```python
# Example: Implementing VALHOOK protocol
class HookValidator:
    def validate_input(self, stdin_data):
        # V - Validate JSON input
        try:
            data = json.loads(stdin_data)
        except json.JSONDecodeError as e:
            self.handle_parse_error(e)  # H - Handle parse errors
            return False

        # A - Assert required fields
        required = ['session_id', 'tool_name', 'hook_event_name']
        if not all(field in data for field in required):
            self.output_structured_error("Missing required fields")  # O - Output structured
            return False

        # L - Link to configuration
        if not self.load_config(data.get('hook_event_name')):
            return False

        # K - Kill on critical
        if data.get('critical_error'):
            sys.exit(2)  # Blocking exit

        return True
```

```python
# Example: Implementing HOOKPIPE protocol
class HookPipeline:
    def process(self):
        # H - Handle stdin JSON
        try:
            input_data = json.load(sys.stdin)
        except json.JSONDecodeError:
            self.emit_error("Invalid JSON input")
            sys.exit(1)

        # O - Output structured data
        output = {
            "hookSpecificOutput": {
                "hookEventName": input_data.get('hook_event_name'),
                "result": None
            }
        }

        # O - Operation atomic
        with self.atomic_operation():
            # K - Kill on timeout
            with timeout(5):
                result = self.process_hook_logic(input_data)
                output['result'] = result

        # P - Parse errors handled
        # I - Interface versioned
        # P - Protocol documented
        # E - Emit metrics

        print(json.dumps(output))
        sys.exit(0)
```

---

## Hook Protocol Integration

### Combining Multiple Protocols

```python
class CompleteHook:
    def execute(self):
        # INIT - Initialize hook
        self.INIT()

        # VALHOOK - Validate input
        if not self.VALHOOK():
            self.HOOKERR()
            return

        # EXEC - Execute main logic
        result = self.EXEC()

        # VERIFY - Verify results
        if not self.VERIFY(result):
            self.FAIL()
            return

        # EMIT - Emit results
        self.EMIT(result)

        # METRICS - Log performance
        self.METRICS()
```

---

## Key Principles

- **No ambiguity**: Every component has a specific, measurable definition
- **Composable**: Acronyms build upon each other
- **Testable**: Success/failure can be determined objectively
- **Versionable**: Protocols can evolve while maintaining backward compatibility
- **Domain-specific**: Acronyms are organized by context and use case
- **Hook-aware**: Protocols specifically designed for Claude Code hook development
