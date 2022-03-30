FROM rabbitmq:3.7-management
RUN rabbitmq-plugins enable --offline rabbitmq_stomp

RUN printf '#!/bin/bash\n\
loopback_users.guest = false\n\
listeners.tcp.default = 5672\n\
log.console.level = debug\n\
log.file = rabbit.log\n\
log.dir = /var/log/rabbitmq\n\
log.file.level = debug\n\
log.connection.level = debug\n\
log.channel.level = debug\n\
log.queue.level = debug\n'\
>> /etc/rabbitmq/rabbitmq.conf


