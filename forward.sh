PATH=/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:/usr/local/sbin:~/bin
export PATH

#=================================================
#	System Required: Debian/Ubuntu
#	Description: forwarding hypixel server
#	Version: 1.0
#	Author: GreepAr
# 魔改自:https://doub.io/wlzy-20/
#=================================================
sh_ver="1.0"
Green_font_prefix="\033[32m" && Red_font_prefix="\033[31m" && Font_color_suffix="\033[0m"


install_iptables(){
	iptables_exist=$(iptables -V)
	if [[ ${iptables_exist} != "" ]]; then
		echo -e "${Info} 已经安装iptables，继续..."
	else
		echo -e "${Info} 检测到未安装 iptables，开始安装..."
		apt-get update
		apt-get install -y iptables
		iptables_exist=$(iptables -V)
		if [[ ${iptables_exist} = "" ]]; then
			echo -e "${Error} 安装iptables失败，请检查 !" && exit 1
		else
			echo -e "${Info} iptables 安装完成 !"
        fi
    fi
	echo -e "${Info} 开始配置 iptables !"
	Set_iptables
	echo -e "${Info} iptables 配置完毕 !"
}
Set_iptables(){
	echo -e "net.ipv4.ip_forward=1" >> /etc/sysctl.conf
	sysctl -p
	iptables-save > /etc/iptables.up.rules
	echo -e '#!/bin/bash\n/sbin/iptables-restore < /etc/iptables.up.rules' > /etc/network/if-pre-up.d/iptables
	chmod +x /etc/network/if-pre-up.d/iptables
}
get_hypixelip(){
    forwarding_ip=$(getent ahosts mc.hypixel.net | awk '{print $1; exit}')
    echo "获取到Hypixel服务器IP地址:$forwarding_ip"
}
Add_forwarding(){
    install_iptables
    get_hypixelip
    local_ip=$(wget -qO- -t1 -T2 ipinfo.io/ip)
        read -p "请输入本地监听端口 (直接回车默认为 25565): " custom_local_port
    local_port=${custom_local_port:-25565}
    iptables -t nat -A PREROUTING -p tcp --dport "${local_port}" -j DNAT --to-destination "${forwarding_ip}":25565
	iptables -t nat -A POSTROUTING -p tcp -d "${forwarding_ip}" --dport "${local_port}" -j SNAT --to-source "${local_ip}"
    iptables -I INPUT -m state --state NEW -m tcp -p tcp --dport "${local_port}" -j ACCEPT
	iptables-save > /etc/iptables.up.rules
    ufw disable
	echo && echo -e "——————————————————————————————
	端口转发规则配置完成 !\n
	游戏内输入的服务器 IP\t: ${Green_font_prefix}${local_ip}:${local_port}${Font_color_suffix}\n
——————————————————————————————\n"
}
Del_forwarding(){
    if ! command -v iptables >/dev/null 2>&1; then
        echo -e "\033[0;31m[错误]\033[0m 系统未安装 iptables，无法删除转发规则。"
        return 1
    fi
    read -p "请输入要删除的本地监听端口 (直接回车默认为 25565): " custom_local_port
    local_port=${custom_local_port:-25565}
    echo -e "\n开始删除端口 ${local_port} 的转发规则…\n"
    prerules=$(iptables -t nat -L PREROUTING --line-numbers \
        | grep DNAT | grep "dpt:${local_port}" | awk '{print $1}' | sort -r -n)
    if [ -n "$prerules" ]; then
        for num in $prerules; do
            iptables -t nat -D PREROUTING "$num" \
            && echo "删除 PREROUTING 规则 #${num}"
        done
    else
        echo "未找到 PREROUTING 中的端口 ${local_port} 的 DNAT 规则"
    fi
    postrules=$(iptables -t nat -L POSTROUTING --line-numbers \
        | grep SNAT | grep "dpt:${local_port}" | awk '{print $1}' | sort -r -n)
    if [ -n "$postrules" ]; then
        for num in $postrules; do
            iptables -t nat -D POSTROUTING "$num" \
            && echo "删除 POSTROUTING 规则 #${num}"
        done
    else
        echo "未找到 POSTROUTING 中的端口 ${local_port} 的 SNAT 规则"
    fi
    inprules=$(iptables -L INPUT --line-numbers \
        | grep ACCEPT | grep "dpt:${local_port}" | awk '{print $1}' | sort -r -n)
    if [ -n "$inprules" ]; then
        for num in $inprules; do
            iptables -D INPUT "$num" \
            && echo "删除 INPUT 规则 #${num}"
        done
    else
        echo "未找到 INPUT 中的端口 ${local_port} 的 ACCEPT 规则"
    fi
    ufw delete allow "${local_port}/tcp" \
    && echo "删除了 ufw allow ${local_port}/tcp 规则"
    iptables-save > /etc/iptables.up.rules
    echo -e "\n 端口 ${local_port} 的转发规则已全部删除完成。\n"
}

if [ "$EUID" -ne 0 ]; then
  echo "请使用 sudo 或以 root 用户身份运行此脚本！"
  exit 1
fi

echo -e " Hypixel服务器一键转发脚本(一键iptables) ${Red_font_prefix}[v${sh_ver}]${Font_color_suffix}
  -- greepar | https://github.com/greepar/HypForward --
  -- 请注意本脚本仅支持Debian以及Debian类系统如Ubuntu --

————————————
 ${Green_font_prefix}1.${Font_color_suffix} 安装服务
 ${Green_font_prefix}2.${Font_color_suffix} 卸载服务
————————————
"

read -e -p " 请输入数字 [1-2]: " num

case "$num" in
	1)
	Add_forwarding
	;;
	2)
	Del_forwarding
	;;
	*)
	echo "请输入正确数字 [1-2]"
	;;
esac
