# Architecture

## Overview

Ironfish follows a **peer-to-peer** architecture where all nodes are capable of performing analysis and serving API requests. The cluster maintains state consistency using a **Gossip Protocol** and achieves high availability through **Leader Election**.

## Core Components

### 1. Networking & Discovery
*   **Gossip Protocol:** Uses a random-peer gossip mechanism to disseminate cluster state (membership, health, load).
*   **Discovery:**
    *   `Static`: Hardcoded list of peers (good for simple setups).
    *   `Multicast`: UDP discovery for local networks.
    *   `DNS`: Resolves SRV/A records to find peers (ideal for Kubernetes Headless Services).

### 2. Consensus (Hybrid)
*   **Bully Algorithm:** Used for initial leader election due to its speed in small, stable clusters.
*   **Raft-like Terms:** Implements "Terms" to prevent split-brain scenarios and ensure strictly increasing versioning of the cluster state.

### 3. Load Balancing
*   **CpuAware:** Nodes broadcast their CPU usage and Queue depth via gossip.
*   **Selection:** The entry node selects the best peer (lowest score based on CPU + Queue + Latency) to forward the analysis request to.

### 4. Engine Management
*   **Stockfish Pool:** Each node manages a local pool of Stockfish processes.
*   **Zombie Killer:** A background task monitors child processes and restarts them if they become unresponsive or die unexpectedly.
