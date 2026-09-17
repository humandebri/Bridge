---
status: accepted
---

# Create bSNS in the Bridge constructor

The Bridge contract creates bSNS in its constructor and immutably assigns supply authority to the creating Bridge. Reject a separate post-deployment initializer because it creates a temporarily unconfigured state, and reject predicted addresses because they introduce unnecessary circular dependencies into deployment.
