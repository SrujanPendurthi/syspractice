# Product Requirements Document (PRD): System Design Study App

## 1. Executive Summary
A local-first, high-performance desktop/web application designed to help engineers master system design interviews. Users interact with a canvas workspace to build architecture diagrams (boxes and arrows) while a specialized, locally-hosted AI agent provides real-time critiques, back-of-the-envelope calculations, and structured feedback based on FAANG interview rubrics.

## 2. Target Persona & Core Problem
* **Persona:** Software engineers preparing for senior/staff-level system design loops.
* **Problem:** Existing study tools rely purely on text chat or generic whiteboards without context-aware AI feedback tailored specifically to architectural trade-offs (scalability, consistency, failure modes).

## 3. Tech Stack
* **Frontend:** React, TypeScript, Tailwind CSS, and a performant diagramming/canvas library (e.g., React Flow or tldraw).
* **Backend:** Rust (Actix-web or Axum) managing local processes and state.
* **Database/Storage:** Local SQLite database via `rusqlite` or `sqlx` for persistent storage of past sessions, diagrams, and drill progress.
* **AI Engine:** Local model runner (e.g., MLX / Ollama / llama.cpp) executing a domain-tuned system design critic agent.

## 4. Core Features (MVP Scope)

### A. Interactive Architecture Canvas
* **Drag-and-Drop Editor:** Nodes representing standard infrastructure components (Load Balancers, API Gateways, Microservices, Databases, Caches, Message Queues, CDNs).
* **Connection Wiring:** Labeled directional edges to map data flow and request paths.
* **Export State:** Ability to serialize the canvas graph into a structured JSON/AST format representing the topology.

### B. System Design Scenarios & Drills
* **Predefined Prompts:** Curated scenarios (e.g., URL Shortener, WhatsApp/Chat App, Distributed Rate Limiter, Video Streaming).
* **Back-of-the-Envelope Calculator:** Interactive tool for quick calculations (QPS, bandwidth, storage sizing) with instant formula checking.
* **Flashcards:** Core architecture patterns (Sharding, Consistent Hashing, CQRS, Event Sourcing) for rapid conceptual review.

### C. Local AI Critique Agent
* **Topology Analysis:** The local agent ingests the serialized canvas graph + scenario requirements.
* **Rubric Grading:** Evaluates designs against scalability, single points of failure (SPOF), data partitioning choices, and trade-offs.
* **Targeted Interventions:** Generates contextual hints or probing questions ("Where is your fallback if Redis goes down?") rather than just giving away the answer.

## 5. User Journey & Flow
1. **Select Scenario:** User picks a challenge (e.g., "Design WhatsApp") and reads functional/non-functional requirements.
2. **Draft Architecture:** User uses the React Flow canvas to drop components, connect services, and define data stores.
3. **Trigger Evaluation:** User requests a critique. The frontend serializes the canvas state and passes it to the Rust backend.
4. **Local Agent Review:** The local agent parses the topology graph, matches it against expected architecture patterns, and streams a structured critique back to the UI.
5. **Iterate & Learn:** User revises the diagram based on feedback and saves the session to local SQLite history.