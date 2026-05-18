name: Feature Request
description: Suggest an idea for knogg
labels: [enhancement]
body:
  - type: markdown
    attributes:
      value: |
        Thanks for suggesting a feature! Please fill out the fields below.
  - type: textarea
    id: problem
    attributes:
      label: Problem statement
      description: Is your feature request related to a problem? Describe it.
      placeholder: I'm always frustrated when ...
    validations:
      required: true
  - type: textarea
    id: solution
    attributes:
      label: Proposed solution
      description: Describe what you want to happen.
    validations:
      required: true
  - type: textarea
    id: alternatives
    attributes:
      label: Alternatives considered
      description: Any alternative solutions or features you've considered.
  - type: dropdown
    id: scope
    attributes:
      label: Scope
      description: Which area does this affect?
      options:
        - CLI (new subcommand or flag)
        - MCP server (new tool or protocol change)
        - Vault (state format or layout)
        - Agent brokering (handoff or sync)
        - Documentation
        - Other
    validations:
      required: true
