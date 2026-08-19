FROM nginx:alpine

ARG SHADOWSOCKS_PORT=443

COPY modules-enabled/ /etc/nginx/modules-enabled/
COPY nginx.conf /etc/nginx/nginx.conf

EXPOSE ${SHADOWSOCKS_PORT}
