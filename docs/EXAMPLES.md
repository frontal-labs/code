# Examples Guide

This guide provides practical examples of using Frontal Code CLI for various tasks and workflows.

## Quick Start Examples

### Basic Usage

```bash
# Simple prompt
frontal-code prompt "What files are in the current directory?"

# Read and analyze a file
frontal-code prompt "Read the README.md file and summarize it"

# Code analysis
frontal-code prompt "Explain what this Rust function does: $(cat src/main.rs)"

# Generate code
frontal-code prompt "Write a Python function that sorts a list of numbers"
```

### Interactive REPL

```bash
# Start interactive session
frontal-code repl

# Use model aliases
frontal-code --model opus repl
frontal-code --model sonnet repl
frontal-code --model haiku repl

# Resume previous session
frontal-code --resume latest repl
```

## File Operations

### Reading Files

```bash
# Read single file
frontal-code prompt "Read package.json and show the dependencies"

# Read multiple files
frontal-code prompt "Read all .md files in the docs directory and summarize them"

# Read with pattern matching
frontal-code prompt "Use glob to find all Rust files, then read and analyze them"

# Read configuration files
frontal-code prompt "Read .env.example and show all environment variables"
```

### Writing Files

```bash
# Create new file
frontal-code prompt "Create a new Python script called hello.py that prints 'Hello, World!'"

# Write configuration
frontal-code prompt "Create a docker-compose.yml file for a web application with nginx and postgres"

# Generate documentation
frontal-code prompt "Read the source code and generate API documentation in api.md"

# Create multiple files
frontal-code prompt "Create a complete React project structure with all necessary files"
```

### Editing Files

```bash
# Edit existing file
frontal-code prompt "Edit the README.md file to add installation instructions"

# Update configuration
frontal-code prompt "Update the Cargo.toml file to add new dependencies"

# Refactor code
frontal-code prompt "Refactor this JavaScript file to use modern ES6 syntax"

# Fix issues
frontal-code prompt "Fix the syntax errors in this Python file"
```

## Code Analysis and Generation

### Code Review

```bash
# Review code quality
frontal-code prompt "Review this code for security vulnerabilities and best practices"

# Performance analysis
frontal-code prompt "Analyze this code for performance bottlenecks and suggest optimizations"

# Code style
frontal-code prompt "Check if this code follows the project's coding standards"

# Documentation review
frontal-code prompt "Review the code comments and suggest improvements"
```

### Code Generation

```bash
# Generate boilerplate
frontal-code prompt "Generate a complete Express.js server with authentication middleware"

# Create API endpoints
frontal-code prompt "Create REST API endpoints for user management with CRUD operations"

# Generate tests
frontal-code prompt "Write unit tests for this Python function using pytest"

# Create configuration
frontal-code prompt "Generate a Kubernetes deployment file for a Node.js application"
```

### Refactoring

```bash
# Extract functions
frontal-code prompt "Extract repeated code into reusable functions"

# Improve structure
frontal-code prompt "Refactor this code to follow the SOLID principles"

# Optimize algorithms
frontal-code prompt "Optimize this sorting algorithm for better performance"

# Modernize code
frontal-code prompt "Modernize this legacy code to use current best practices"
```

## System Administration

### File System Management

```bash
# Clean up temporary files
frontal-code prompt "Find and remove all temporary files older than 7 days"

# Organize directories
frontal-code prompt "Organize the downloads directory by file type into subdirectories"

# Disk usage analysis
frontal-code prompt "Analyze disk usage and identify the largest files and directories"

# Backup files
frontal-code prompt "Create a backup script that copies important files to a backup location"
```

### Process Management

```bash
# Monitor system resources
frontal-code prompt "Check system resource usage and identify processes consuming high CPU"

# Kill processes
frontal-code prompt "Find and terminate all processes matching a specific pattern"

# Service management
frontal-code prompt "Check the status of all system services and restart any that are failed"

# Log analysis
frontal-code prompt "Analyze system logs to identify errors and warnings"
```

### Network Operations

```bash
# Network diagnostics
frontal-code prompt "Run network diagnostics to check connectivity and identify issues"

# Port scanning
frontal-code prompt "Scan for open ports on the local system and identify running services"

# Bandwidth monitoring
frontal-code prompt "Monitor network bandwidth usage by process"

# DNS troubleshooting
frontal-code prompt "Troubleshoot DNS resolution issues and verify configuration"
```

## Web Development

### Frontend Development

```bash
# Create React components
frontal-code prompt "Create a React component for a user profile page with avatar and details"

# CSS generation
frontal-code prompt "Generate CSS for a responsive navigation menu with hover effects"

# JavaScript utilities
frontal-code prompt "Write JavaScript utility functions for form validation and API calls"

# Build configuration
frontal-code prompt "Create a webpack configuration for a modern JavaScript application"
```

### Backend Development

```bash# API development
frontal-code prompt "Create a REST API using Express.js with user authentication and JWT"

# Database operations
frontal-code prompt "Write SQL queries to create a user table with proper indexes"

# API documentation
frontal-code prompt "Generate OpenAPI documentation for the existing API endpoints"

# Error handling
frontal-code prompt "Implement comprehensive error handling for a Node.js application"
```

### DevOps Tasks

```bash
# Docker setup
frontal-code prompt "Create a Dockerfile for a Node.js application with multi-stage build"

# CI/CD pipeline
frontal-code prompt "Write a GitHub Actions workflow for automated testing and deployment"

# Infrastructure as code
frontal-code prompt "Create Terraform configuration for deploying a web application on AWS"

# Monitoring setup
frontal-code prompt "Set up Prometheus and Grafana monitoring for a web application"
```

## Data Processing

### Log Analysis

```bash
# Parse logs
frontal-code prompt "Parse Apache access logs and extract IP addresses, timestamps, and status codes"

# Analyze patterns
frontal-code prompt "Analyze web server logs to identify the most requested pages and error rates"

# Generate reports
frontal-code prompt "Create a daily report of system activities from log files"

# Filter and search
frontal-code prompt "Filter log files to show only error messages from the last 24 hours"
```

### Data Transformation

```bash
# CSV processing
frontal-code prompt "Read a CSV file and transform it into JSON format"

# Data cleaning
frontal-code prompt "Clean up a dataset by removing duplicates and fixing formatting issues"

# Data aggregation
frontal-code prompt "Aggregate sales data by month and calculate totals and averages"

# Format conversion
frontal-code prompt "Convert XML data to JSON format and validate the output"
```

### Text Processing

```bash
# Text extraction
frontal-code prompt "Extract email addresses and phone numbers from a text file"

# Content analysis
frontal-code prompt "Analyze text content for sentiment and key topics"

# Text generation
frontal-code prompt "Generate product descriptions based on feature lists"

# Translation
frontal-code prompt "Translate a document from English to Spanish while preserving formatting"
```

## Automation Workflows

### File Automation

```bash
# Batch processing
frontal-code prompt "Process all images in a directory to resize and optimize them"

# File organization
frontal-code prompt "Automatically organize files by date and type into appropriate directories"

# Content generation
frontal-code prompt "Generate HTML pages from Markdown files in a directory"

# Backup automation
frontal-code prompt "Create an automated backup system that runs daily and sends reports"
```

### Task Automation

```bash
# Email automation
frontal-code prompt "Create a script to send daily summary emails with system statistics"

# Report generation
frontal-code prompt "Generate weekly reports from data files and email them to stakeholders"

# Data synchronization
frontal-code prompt "Synchronize data between two different systems and handle conflicts"

# Scheduled tasks
frontal-code prompt "Set up a cron job to run maintenance tasks automatically"
```

## Testing and Quality Assurance

### Test Generation

```bash
# Unit tests
frontal-code prompt "Write comprehensive unit tests for a Python class using pytest"

# Integration tests
frontal-code prompt "Create integration tests for a REST API using Postman/Newman"

# Performance tests
frontal-code prompt "Write load testing scripts using Apache Bench for a web application"

# Test data generation
frontal-code prompt "Generate realistic test data for a database with proper relationships"
```

### Code Quality

```bash
# Linting configuration
frontal-code prompt "Configure ESLint for a JavaScript project with custom rules"

# Code coverage
frontal-code prompt "Set up code coverage reporting for a Python project"

# Security scanning
frontal-code prompt "Configure security scanning tools and review the results"

# Documentation generation
frontal-code prompt "Generate API documentation from code comments and type annotations"
```

## Development Workflows

### Git Operations

```bash
# Git workflow
frontal-code prompt "Create a feature branch, make changes, commit, and create a pull request"

# Commit message generation
frontal-code prompt "Generate descriptive commit messages based on the changes made"

# Merge conflict resolution
frontal-code prompt "Help resolve merge conflicts in a Git repository"

# Release management
frontal-code prompt "Create a release branch, update version numbers, and tag the release"
```

### Project Setup

```bash
# Project initialization
frontal-code prompt "Initialize a new Python project with proper structure and dependencies"

# Environment setup
frontal-code prompt "Set up a development environment with Docker and necessary tools"

# Configuration management
frontal-code prompt "Create configuration files for different environments (dev, staging, prod)"

# Documentation setup
frontal-code prompt "Set up project documentation with README, contributing guidelines, and API docs"
```

## Advanced Examples

### Complex Workflows

```bash
# Multi-step analysis
frontal-code prompt "Read all source code files, analyze dependencies, create a dependency graph, and suggest refactoring opportunities"

# System optimization
frontal-code prompt "Analyze system performance, identify bottlenecks, and implement optimizations"

# Security audit
frontal-code prompt "Perform a comprehensive security audit of the codebase and generate a report with recommendations"

# Migration planning
frontal-code prompt "Plan the migration of a monolithic application to microservices architecture"
```

### Integration Examples

```bash
# GitHub integration
frontal-code prompt "Use GitHub API to list repositories, analyze code, and create issues for improvements"

# Database integration
frontal-code prompt "Connect to a PostgreSQL database, analyze schema, and generate documentation"

# Cloud integration
frontal-code prompt "Use AWS CLI to list resources, analyze costs, and generate optimization recommendations"

# API integration
frontal-code prompt "Integrate with multiple APIs to aggregate data and generate a comprehensive report"
```

## Real-World Scenarios

### E-commerce Platform

```bash
# Product catalog management
frontal-code prompt "Create a system to manage product catalogs with categories, pricing, and inventory"

# Order processing
frontal-code prompt "Implement order processing workflow with payment integration and inventory updates"

# Customer management
frontal-code prompt "Build a customer management system with profiles, orders, and support tickets"

# Analytics dashboard
frontal-code prompt "Create an analytics dashboard to track sales, customers, and product performance"
```

### Content Management System

```bash
# Content creation
frontal-code prompt "Build a content creation system with rich text editing and media management"

# User management
frontal-code prompt "Implement user authentication, roles, and permissions for content access"

# SEO optimization
frontal-code prompt "Add SEO optimization features including meta tags, sitemaps, and URL structure"

# Performance optimization
frontal-code prompt "Optimize the CMS for performance with caching and database optimization"
```

### Data Science Workflow

```bash
# Data collection
frontal-code prompt "Set up automated data collection from multiple sources and store in database"

# Data analysis
frontal-code prompt "Analyze collected data using statistical methods and machine learning"

# Visualization
frontal-code prompt "Create interactive visualizations and dashboards for data insights"

# Reporting
frontal-code prompt "Generate automated reports with insights and recommendations"
```

## Tips and Best Practices

### Effective Prompting

```bash
# Be specific
frontal-code prompt "Read the package.json file and list all dependencies with their versions"

# Provide context
frontal-code prompt "This is a React project. Read the components and suggest improvements for performance"

# Use examples
frontal-code prompt "Create a function similar to this example: [provide code example]"

# Break down complex tasks
frontal-code prompt "First, analyze the current code structure. Then, identify areas for improvement. Finally, implement changes"
```

### Error Handling

```bash
# Handle failures gracefully
frontal-code prompt "Try to read the configuration file. If it doesn't exist, create a default one"

# Validate inputs
frontal-code prompt "Validate the user input before processing and show appropriate error messages"

# Retry logic
frontal-code prompt "Implement retry logic for API calls with exponential backoff"

# Logging
frontal-code prompt "Add comprehensive logging to track application behavior and debug issues"
```

### Performance Optimization

```bash
# Use appropriate models
frontal-code --model haiku prompt "Simple task that doesn't require complex reasoning"
frontal-code --model sonnet prompt "Moderate complexity task requiring some analysis"
frontal-code --model opus prompt "Complex task requiring deep analysis and creativity"

# Batch operations
frontal-code prompt "Process multiple files in parallel instead of sequentially"

# Caching
frontal-code prompt "Implement caching for frequently accessed data to improve performance"

# Optimization
frontal-code prompt "Analyze the code and identify performance bottlenecks, then optimize them"
```

This examples guide provides a comprehensive collection of practical use cases for Frontal Code CLI across various domains and skill levels.
