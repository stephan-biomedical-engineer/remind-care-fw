#!/bin/bash
set -e

PI_USER="pi"
PI_HOST="raspberrypi.local" # IP ou Hostname da Pi
PI_DIR="/home/$PI_USER/telemed_os"

echo "⚙️ Compilando para AArch64 (Raspberry Pi OS Lite 64-bit)..."
cargo build --release --target aarch64-unknown-linux-gnu

echo "🛑 Parando o serviço remoto (se estiver rodando)..."
ssh $PI_USER@$PI_HOST "sudo systemctl stop telemed_os || true"

echo "📦 Transferindo arquivos de produção..."
# Se houver um arquivo binário antigo com o mesmo nome da pasta, remove ele primeiro
ssh $PI_USER@$PI_HOST "rm -f $PI_DIR 2>/dev/null || true; mkdir -p $PI_DIR"

scp target/aarch64-unknown-linux-gnu/release/telemed_os $PI_USER@$PI_HOST:$PI_DIR/
scp .env.production $PI_USER@$PI_HOST:$PI_DIR/
scp telemed_os.service $PI_USER@$PI_HOST:$PI_DIR/

echo "🔒 Configurando permissões de segurança remotamente..."
ssh $PI_USER@$PI_HOST "chmod 600 $PI_DIR/.env.production && chmod +x $PI_DIR/telemed_os"

echo "🚀 Registrando e reiniciando o serviço no Systemd..."
ssh $PI_USER@$PI_HOST "
  sudo cp $PI_DIR/telemed_os.service /etc/systemd/system/
  sudo systemctl daemon-reload
  sudo systemctl enable telemed_os
  sudo systemctl restart telemed_os
"

echo "✅ Deploy de Produção concluído com Sucesso Total!"
echo "📡 Acompanhe os logs na placa com:"
echo "ssh $PI_USER@$PI_HOST 'journalctl -fu telemed_os'"
