name: Bug Report
description: Report a bug in knogg
labels: [bug]
body:
  - type: markdown
    attributes:
      value: |
        Thanks for taking the time to fill out this bug report!
  - type: input
    id: version
    attributes:
      label: knogg version
      description: Output of `knogg --version` or `./knogg --version`
      placeholder: e.g. 1.0.0
    validations:
      required: true
  - type: input
    id: os
    attributes:
      label: Operating system
      placeholder: e.g. Ubuntu 24.04, macOS 15.3, Windows 11
    validations:
      required: true
  - type: input
    id: docker
    attributes:
      label: Docker version
      description: Output of `docker --version` and `docker compose version`
      placeholder: e.g. Docker 27.5.1, Compose v2.32.4
  - type: textarea
    id: description
    attributes:
      label: What happened?
      description: Describe the bug and include steps to reproduce.
      placeholder: |
        1. Run `knogg init`
        2. Run `knogg status`
        3. See error: ...
    validations:
      required: true
  - type: textarea
    id: expected
    attributes:
      label: What did you expect to happen?
    validations:
      required: true
  - type: textarea
    id: logs
    attributes:
      label: Relevant output
      description: Paste command output, logs, or error messages.
      render: shell
