# Development

## Objective
Begin planning out the project in a logical, methodical, and strategic manner.

## Core Principles
- Keep implementations simple and complete - complex things must be simplified
- Follow DRY (Don't Repeat Yourself) principles
- Maintain modular, reusable architecture
- Ensure comprehensive documentation and commenting for future development
- All implementations should be thoroughly tested and documented
- Unless its for testing purposes, all implementations should be complete from end to end

## Required Process

### 1. Prerequisites Analysis
- Identify all dependencies and existing components that will be affected
- Document current system state and integration points
- Assess potential conflicts with existing architecture

### 2. Documentation Before Implementation
Before writing any code, create a detailed implementation plan following the structure below.

### 3. Implementation Planning Structure
**File location**: `<project_name>/docs/plans/[feature-name]-implementation.md`
**Distribution**: `<project_name>/docs/tasks/[feature-name]/[filename]that-lists-individual-tasks.jsonl

**Required sections**:
- **Scope**: Exact boundaries of what will be implemented
- **Dependencies**: List all external dependencies and internal components affected
- **File modifications**: Specific files to be created, modified, or deleted
- **Function signatures**: Exact method signatures and interfaces
- **Data structures**: Precise schema and type definitions
- **Step sequence**: Numbered implementation steps in dependency order
- **Testing approach**: How implementation will be verified
- **Rollback plan**: Steps to revert changes if needed

### 4. Implementation Standards
All code implementations must be:
- **Modular**: Components should be independent and interchangeable
- **Reusable**: Code should be designed for future use across different contexts
- **Well-commented**: Include comprehensive inline documentation
- **Well-documented**: Maintain external documentation for all modules and functions

## Execution Flow
1. **Analysis**: Assess requirements and current system state
2. **Planning**: Create implementation plan with all required sections
3. **Review**: Verify plan completeness against checklist:
   - [ ] All dependencies identified
   - [ ] File changes specified
   - [ ] Step sequence is dependency-ordered
   - [ ] Testing approach defined
   - [ ] Rollback plan exists
4. **Implementation**: Execute following documented plan exactly
5. **Verification**: Test implementation against defined criteria
6. **Documentation update**: Update relevant docs to reflect changes

## Quality Gates
- **During implementation**: Each step must be completed before proceeding to next

## Decision Framework
For architecture choices, document:
- **Problem**: What specific issue is being solved
- **Options**: Alternative approaches considered
- **Choice**: Selected approach with reasoning
- **Trade-offs**: What is gained/lost with this decision

## Output Format
- **Implementation plans**: Markdown format in designated directory with standardized naming
- **Code**: Follow established project conventions with comprehensive comments
- **Documentation**: Clear, factual, and practical
- **Commit messages**: Reference implementation plan file for traceability

## Complexity Management
- **Single responsibility**: Each implementation plan should address one logical unit of work
- **Size limit**: If implementation plan exceeds 20 steps, break into multiple plans
- **Dependency mapping**: Create separate plans for components with different dependencies
- **Incremental delivery**: Structure plans to enable partial implementation and testing
