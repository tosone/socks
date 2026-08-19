FROM nginx:alpine

ARG SHADOWSOCKS_PORT=39036
ENV SHADOWSOCKS_PORT=${SHADOWSOCKS_PORT}

COPY nginx.conf /etc/nginx/nginx.conf
COPY docker-entrypoint.sh /usr/local/bin/socks-proxy-entrypoint.sh

RUN chmod +x /usr/local/bin/socks-proxy-entrypoint.sh

EXPOSE ${SHADOWSOCKS_PORT}

ENTRYPOINT ["socks-proxy-entrypoint.sh"]
CMD ["nginx", "-g", "daemon off;"]

# docker buildx build --platform linux/amd64,linux/arm64 -f build/socks-proxy/Dockerfile -t socks-proxy:latest build/socks-proxy
# docker run --rm -e SHADOWSOCKS_SERVER=example.com -e SHADOWSOCKS_PORT=39036 -p 39036:39036/tcp -p 39036:39036/udp socks-proxy:latest
# docker run --rm -e "SHADOWSOCKS_SERVER=example-a.com example-b.com" -e SHADOWSOCKS_PORT=39036 -p 39036:39036/tcp -p 39036:39036/udp socks-proxy:latest
