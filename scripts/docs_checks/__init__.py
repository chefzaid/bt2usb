"""The checks behind scripts/check_docs.py, one module per check.

``model`` reads a Markdown file into prose, code spans, and fenced lines and
holds the ``Finding`` every check reports; ``links``, ``config``, ``memory``,
``commands``, and ``counts`` (the ``tests`` check) each export a
``check(root, docs)`` function.
"""
