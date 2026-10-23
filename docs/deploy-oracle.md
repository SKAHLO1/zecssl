# Deploying ZEC SSL on Oracle Cloud (free) and Vercel

The backend (coordinator, three operators, read API, HTTPS) runs on one Oracle Cloud Always Free VM with Docker Compose. The website runs on Vercel's free Hobby plan.

| Piece | Where | Cost |
| --- | --- | --- |
| Coordinator, operators 1–3, API, Caddy (HTTPS) | Oracle Ampere A1 VM | Always Free |
| Website | Vercel Hobby | Free |
| API domain | `<your-ip-with-dashes>.sslip.io` | Free, no domain to buy |

All three operators on one VM is fine for testnet. With real funds, each operator runs on its own machine, held by a different party.

## 1. Create the VM

1. Oracle Cloud console → **Compute → Instances → Create instance**.
2. **Image:** Canonical Ubuntu 24.04. **Shape:** Ampere → `VM.Standard.A1.Flex`, **4 OCPUs, 24 GB memory**. That's the whole free allowance, and the first build needs the memory.
   - If Oracle reports "out of capacity", try 2 OCPU / 12 GB or another availability domain, and retry later.
3. **Networking:** keep the default new VCN with a public subnet, and assign a public IPv4 address.
4. **SSH keys:** "Generate a key pair for me" → download the private key (for example to `C:\Users\USER\.ssh\oracle.key`).
5. **Boot volume:** 100 GB (free up to 200 GB). Create.
6. Note the **public IP**, for example `152.67.10.20`. Your API domain is then `152-67-10-20.sslip.io`.

## 2. Open ports 80 and 443

Two firewalls are involved: Oracle's network and the VM's own.

1. Console → the instance → **Subnet** → **Security Lists** → default list → **Add Ingress Rules**:
   - Source `0.0.0.0/0`, TCP, destination port `80`;
   - again for port `443`.
2. On the VM (step 3 shows how to connect):
   ```bash
   sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 80 -j ACCEPT
   sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 443 -j ACCEPT
   sudo netfilter-persistent save
   ```

## 3. Connect and install Docker

From Windows PowerShell:

```bash
ssh -i C:\Users\USER\.ssh\oracle.key ubuntu@152.67.10.20
```

If SSH complains the key is too open, run `icacls C:\Users\USER\.ssh\oracle.key /inheritance:r /grant:r "$env:USERNAME:R"` once.

On the VM:

```bash
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker ubuntu
exit
```

Then reconnect so the group change applies.

## 4. Get the code onto the VM

Push the repository to GitHub (a private repository is fine). Then on the VM:

```bash
git clone https://github.com/<you>/zec-ssl.git
```

For a private repository, use a GitHub fine-grained token with read access to contents as the password.

## 5. Make the secrets file (on your PC, in WSL)

The keys live in WSL: the operators' NEAR keys, the FROST shares from the DKG, and the treasury viewing key. This step collects them into one file:

```bash
wsl -d Ubuntu -- bash -lc "cd /mnt/c/Users/USER/Downloads/zec-ssl && PREFIX=zecssl-skahlo API_DOMAIN=152-67-10-20.sslip.io WEB_ORIGIN=https://zec-ssl.vercel.app bash deploy/make-env.sh"
```

Use your Vercel URL for `WEB_ORIGIN` if you know it. If not, fix it in step 8.

Copy the file to the VM, then delete the local copy:

```bash
scp -i C:\Users\USER\.ssh\oracle.key C:\Users\USER\Downloads\zec-ssl\deploy\.env ubuntu@152.67.10.20:zec-ssl/deploy/.env
```

`deploy/.env` holds every secret of the deployment and is git-ignored. Never commit it or paste it anywhere.

## 6. Stop the local services and carry over their state

**Only one copy of each operator may run.** Two processes with the same NEAR key race for nonces. Stop `start-local.sh` and run `wsl --shutdown`.

Optionally, move the local state so open orders (and pending refunds) carry on. On your PC:

```bash
wsl -d Ubuntu -- bash -lc "cd ~ && tar czf /mnt/c/Users/USER/zec-ssl-state.tgz -C zec-ssl-run coordinator op1 op2 op3"
scp -i C:\Users\USER\.ssh\oracle.key C:\Users\USER\zec-ssl-state.tgz ubuntu@152.67.10.20:
```

On the VM, after step 7's first `docker compose up` (which creates the volumes), load the state into them:

```bash
cd ~/zec-ssl/deploy && docker compose stop
mkdir -p ~/state && tar xzf ~/zec-ssl-state.tgz -C ~/state
for s in coordinator:coordinator op1:operator-1 op2:operator-2 op3:operator-3; do
  docker run --rm -v ~/state/${s%%:*}:/from -v deploy_${s##*:}:/to alpine sh -c 'cp -a /from/. /to/'
done
docker compose up -d
```

Skip this step to start fresh. Orders opened locally then won't settle; let them expire first.

## 7. Start everything

On the VM:

```bash
cd ~/zec-ssl/deploy
docker compose up -d --build
```

The first build compiles the Zcash and FROST libraries and takes 20–40 minutes on the ARM VM. Later builds reuse the cache.

To follow progress and check health:

```bash
docker compose ps
docker compose logs -f coordinator operator-1
curl https://152-67-10-20.sslip.io/v1/health
```

The services need a minute or two after first start to import the viewing key and sync the wallets.

## 8. Website on Vercel

1. **New Project** → import the repository. **Root Directory:** `web`. Leave "Include files outside the root directory" on.
2. Set these environment variables for Production and Preview:
   ```
   NEXT_PUBLIC_API_URL=https://152-67-10-20.sslip.io
   API_URL=https://152-67-10-20.sslip.io
   ```
3. Deploy, and note the URL.
4. If `CORS_ORIGINS` in `deploy/.env` doesn't match that URL, edit it on the VM and apply the change:
   ```bash
   cd ~/zec-ssl/deploy && nano .env && docker compose up -d api
   ```

## Day-to-day

| Task | Command (on the VM, in `~/zec-ssl/deploy`) |
| --- | --- |
| Update to the latest code | `git pull && docker compose up -d --build` |
| Logs | `docker compose logs -f --tail 100 <service>` |
| Restart one service | `docker compose restart operator-2` |
| Stop / start everything | `docker compose down` / `docker compose up -d` (volumes are kept) |
| Back up state | `for v in coordinator operator-1 operator-2 operator-3; do docker run --rm -v deploy_$v:/v -v ~/backup:/b alpine tar czf /b/$v-$(date +%F).tgz -C /v .; done` |

- **Back up the operator volumes regularly.** An operator that loses its state forgets which orders it approved and which refunds it signed. Its wallet rebuilds from the chain; the rest doesn't.
- **Never run `docker compose down -v`.** The `-v` deletes the volumes.
- The Always Free VM can be reclaimed if it sits idle for long stretches. These services keep it busy, so that shouldn't happen.
